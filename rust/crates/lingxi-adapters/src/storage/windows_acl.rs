//! Windows 私有数据文件的统一 ACL 边界。新对象在创建时设 DACL，旧对象按句柄收紧。

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AclSizeInformation, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorControl,
    GetSecurityDescriptorDacl, GetTokenInformation, IsValidSid, TokenOwner, TokenUser,
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, TOKEN_INFORMATION_CLASS, TOKEN_OWNER, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    value.push(0);
    Ok(value)
}

fn sid_string(value: windows_sys::Win32::Security::PSID) -> io::Result<String> {
    if value.is_null() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "SID missing"));
    }
    let mut sid: *mut u16 = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(value, &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if sid.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SID string missing",
        ));
    }
    let mut length = 0usize;
    while unsafe { *sid.add(length) } != 0 {
        length += 1;
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(sid, length) })
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SID string"));
    unsafe { LocalFree(sid.cast()) };
    result
}

fn current_token_sid(class: TOKEN_INFORMATION_CLASS) -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if token.is_null() || token == INVALID_HANDLE_VALUE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process token missing",
        ));
    }
    let result = (|| {
        let mut bytes = 0u32;
        unsafe { GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut bytes) };
        let min = if class == TokenUser {
            std::mem::size_of::<TOKEN_USER>()
        } else {
            std::mem::size_of::<TOKEN_OWNER>()
        };
        if bytes < min as u32 || bytes > 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid token user size",
            ));
        }
        // TOKEN_USER 的内存必须按指针宽度对齐，不能转用 Vec<u8>。
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), bytes, &mut bytes)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let sid = if class == TokenUser {
            unsafe { (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid }
        } else {
            unsafe { (*(buffer.as_ptr().cast::<TOKEN_OWNER>())).Owner }
        };
        sid_string(sid)
    })();
    unsafe { CloseHandle(token) };
    result
}

fn current_user_sid() -> io::Result<String> {
    current_token_sid(TokenUser)
}

fn check_owner(file: &File) -> io::Result<()> {
    let mut owner = std::ptr::null_mut();
    let mut security: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut security,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let actual = sid_string(owner);
    unsafe { LocalFree(security.cast()) };
    if actual? != current_token_sid(TokenOwner)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private path has foreign owner",
        ));
    }
    Ok(())
}

struct PrivateDescriptor(PSECURITY_DESCRIPTOR);

impl PrivateDescriptor {
    fn new(directory: bool) -> io::Result<Self> {
        let sid = current_user_sid()?;
        // D:P 阻断父目录的宽松继承；目录 ACE 向其未来子项传播。
        let flags = if directory { "OICI" } else { "" };
        let sddl = format!("D:P(A;{flags};FA;;;{sid})(A;{flags};FA;;;SY)");
        Self::parse(&sddl)
    }

    fn parse(sddl: &str) -> io::Result<Self> {
        let mut encoded: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded.as_mut_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if descriptor.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "security descriptor missing",
            ));
        }
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }

    fn dacl(&self) -> io::Result<*mut ACL> {
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = std::ptr::null_mut();
        if unsafe { GetSecurityDescriptorDacl(self.0, &mut present, &mut acl, &mut defaulted) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if present == 0 || acl.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "private DACL missing",
            ));
        }
        Ok(acl)
    }
}

impl Drop for PrivateDescriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0.cast()) };
    }
}

/// 只在真实句柄上修改 DACL；拒绝重解析点、错误类型和多硬链接文件。
pub fn protect_handle(file: &File, directory: bool) -> io::Result<()> {
    let meta = file.metadata()?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (directory && !meta.is_dir())
        || (!directory && (!meta.is_file() || meta.number_of_links() != Some(1)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsafe private path",
        ));
    }
    check_owner(file)?;
    let descriptor = PrivateDescriptor::new(directory)?;
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            descriptor.dacl()?,
            std::ptr::null(),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

fn open_path(path: &Path, directory: bool, write_acl: bool) -> io::Result<File> {
    let path = wide(path)?;
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES
                | if write_acl {
                    READ_CONTROL | WRITE_DAC
                } else {
                    FILE_TRAVERSE
                },
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            flags,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn open_for_acl(path: &Path, directory: bool) -> io::Result<File> {
    let _ancestors = hold_directory_chain(
        path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "private path has no parent")
        })?,
        false,
    )?;
    open_path(path, directory, true)
}

fn check_directory(file: &File) -> io::Result<()> {
    let meta = file.metadata()?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsafe parent directory",
        ));
    }
    Ok(())
}

fn get_ace(acl: *const ACL, index: u32) -> io::Result<*const ACCESS_ALLOWED_ACE> {
    let mut ace = std::ptr::null_mut();
    if unsafe { GetAce(acl, index, &mut ace) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let header = unsafe { &*(ace.cast::<ACE_HEADER>()) };
    if header.AceSize < 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "short ACL entry",
        ));
    }
    let sid = unsafe { ace.cast::<u8>().add(8) };
    let sub_authorities = unsafe { *sid.add(1) } as usize;
    if usize::from(header.AceSize) < 16 + 4 * sub_authorities {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated ACL SID",
        ));
    }
    Ok(ace.cast())
}

fn ace_count(acl: *const ACL) -> io::Result<u32> {
    let mut info = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            acl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info.AceCount)
}

pub fn verify_private_handle(file: &File, directory: bool) -> io::Result<()> {
    check_owner(file)?;
    let mut actual_acl = std::ptr::null_mut();
    let mut security: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut actual_acl,
            std::ptr::null_mut(),
            &mut security,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let result = (|| {
        if actual_acl.is_null() || security.is_null() || ace_count(actual_acl)? != 2 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "directory DACL is not private",
            ));
        }
        let mut control = 0;
        let mut revision = 0;
        if unsafe { GetSecurityDescriptorControl(security, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if control & SE_DACL_PROTECTED == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "directory DACL inherits external rights",
            ));
        }
        let expected = PrivateDescriptor::new(directory)?;
        let expected_acl = expected.dacl()?;
        let mut matched = [false; 2];
        for index in 0..2 {
            let actual = unsafe { &*get_ace(actual_acl, index)? };
            let mut found = false;
            for candidate in 0..2 {
                if matched[candidate as usize] {
                    continue;
                }
                let wanted = unsafe { &*get_ace(expected_acl, candidate)? };
                let actual_sid = (&actual.SidStart as *const u32).cast_mut().cast();
                let expected_sid = (&wanted.SidStart as *const u32).cast_mut().cast();
                if actual.Header.AceType == wanted.Header.AceType
                    && actual.Header.AceFlags == wanted.Header.AceFlags
                    && actual.Mask == wanted.Mask
                    && unsafe { IsValidSid(actual_sid) } != 0
                    && unsafe { EqualSid(actual_sid, expected_sid) } != 0
                {
                    matched[candidate as usize] = true;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "directory DACL grants other rights",
                ));
            }
        }
        Ok(())
    })();
    unsafe { LocalFree(security.cast()) };
    result
}

/// 既有备份目录只验证、不修改。守卫保持全路径句柄直到操作结束。
#[derive(Debug)]
pub struct PrivateDirectoryGuard {
    _ancestors: Vec<File>,
    _directory: File,
}

pub fn require_private_directory(path: &Path) -> io::Result<PrivateDirectoryGuard> {
    let ancestors = hold_directory_chain(
        path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "private directory has no parent",
            )
        })?,
        false,
    )?;
    let directory = open_path(path, true, true)?;
    check_directory(&directory)?;
    verify_private_handle(&directory, true)?;
    Ok(PrivateDirectoryGuard {
        _ancestors: ancestors,
        _directory: directory,
    })
}

/// 只为本机客户端读取两个运行记录。目录和目标文件的句柄一直保留到字节读取结束。
pub fn read_private_runtime_json(home: &Path, name: &str) -> io::Result<Vec<u8>> {
    let filename = match name {
        "instance" => "instance.json",
        "local-token" => "local-token.json",
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown runtime record",
            ))
        }
    };
    if !home.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "home must be absolute",
        ));
    }
    let _home_guard = require_private_directory(home)?;
    let runtime = home.join("lingxi-service");
    let _runtime_guard = require_private_directory(&runtime)?;
    let file_path = runtime.join(filename);
    let encoded = wide(&file_path)?;
    // 不分享写入或删除；权限核查与字节读取始终使用这一最终句柄。
    let handle = unsafe {
        CreateFileW(
            encoded.as_ptr(),
            windows_sys::Win32::Foundation::GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_handle(handle) };
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || meta.number_of_links() != Some(1)
        || meta.len() == 0
        || meta.len() > 65536
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsafe runtime record",
        ));
    }
    verify_private_handle(&file, false)?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    (&mut file).take(65537).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    verify_private_handle(&file, false)?;
    if bytes.is_empty()
        || bytes.len() > 65536
        || after.len() != bytes.len() as u64
        || after.number_of_links() != Some(1)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "runtime record size changed",
        ));
    }
    Ok(bytes)
}

// 旧 artifacts 若曾对其他账户开放，内容可能已被改写；只能拒绝，不能先收紧再信任。
pub fn prepare_private_artifacts(home: &Path) -> io::Result<()> {
    if !home.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "home must be absolute",
        ));
    }
    let artifacts = home.join("artifacts");
    if artifacts.exists() {
        require_private_directory(home)?;
        require_private_directory(&artifacts)?;
    } else {
        ensure_private_directory(home)?;
        ensure_private_directory(&artifacts)?;
    }
    for name in ["server", "pointers"] {
        let child = artifacts.join(name);
        if child.exists() {
            require_private_directory(&child)?;
        } else {
            ensure_private_directory(&child)?;
        }
    }
    Ok(())
}

fn checked_artifact_server_root(home: &Path, target: &Path) -> io::Result<()> {
    if !home.is_absolute() || !target.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "artifact path must be absolute",
        ));
    }
    let server = home.join("artifacts").join("server");
    if target.parent() != Some(server.as_path()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "artifact target is not a direct server version",
        ));
    }
    require_private_directory(home)?;
    require_private_directory(&home.join("artifacts"))?;
    require_private_directory(&server)?;
    Ok(())
}

fn walk_private_artifact(
    path: &Path,
    seal: bool,
    depth: usize,
    count: &mut usize,
) -> io::Result<()> {
    if depth > 128 || *count >= 100_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "artifact tree limit exceeded",
        ));
    }
    *count += 1;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "artifact reparse point refused",
        ));
    }
    if meta.is_dir() {
        if seal {
            ensure_private_directory(path)?;
        }
        let _guard = require_private_directory(path)?;
        for child in fs::read_dir(path)? {
            walk_private_artifact(&child?.path(), seal, depth + 1, count)?;
        }
    } else if meta.is_file() && meta.number_of_links() == Some(1) {
        if seal {
            ensure_private_file(path)?;
        }
        let file = open_for_acl(path, false)?;
        let after = file.metadata()?;
        if !after.is_file()
            || after.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || after.number_of_links() != Some(1)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsafe artifact file",
            ));
        }
        verify_private_handle(&file, false)?;
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsafe artifact entry",
        ));
    }
    Ok(())
}

/// 只可封闭已在私有 server 父目录内新解压的临时树，不能接管旧目录。
pub fn seal_private_artifact_tree(home: &Path, target: &Path) -> io::Result<()> {
    checked_artifact_server_root(home, target)?;
    let mut count = 0;
    walk_private_artifact(target, true, 0, &mut count)
}

/// 启动及回退前逐句柄检查全树；任何权限扩大、链接或读取失败都拒绝。
pub fn verify_private_artifact_tree(home: &Path, target: &Path) -> io::Result<()> {
    checked_artifact_server_root(home, target)?;
    let mut count = 0;
    walk_private_artifact(target, false, 0, &mut count)
}

fn create_directory_with_acl(path: &Path) -> io::Result<()> {
    let descriptor = PrivateDescriptor::new(true)?;
    let attributes = descriptor.attributes();
    let encoded = wide(path)?;
    if unsafe { CreateDirectoryW(encoded.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// 从文件系统根开始逐层持有目录句柄，且不分享 DELETE。被持有的祖先不能在
// 最终文件创建/权限核对期间被改名；每层都按句柄拒绝重解析点。
fn hold_directory_chain(path: &Path, create_missing: bool) -> io::Result<Vec<File>> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory must be absolute",
        ));
    }
    let mut chain = Vec::new();
    for component in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if component.as_os_str().is_empty() {
            continue;
        }
        if !component.exists() {
            if !create_missing {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            // 父句柄已被保留；新祖先也从创建时即使用私有 DACL。
            // 若发生同名抢占，CreateDirectoryW 明报 AlreadyExists，不接管该目录。
            create_directory_with_acl(component)?;
        }
        let file = open_path(component, true, false)?;
        check_directory(&file)?;
        chain.push(file);
    }
    Ok(chain)
}

/// 收紧已有文件；调用者须保证其父目录先已收紧。
pub fn ensure_private_file(path: &Path) -> io::Result<()> {
    let file = open_for_acl(path, false)?;
    protect_handle(&file, false)
}

/// 创建或收紧最终目录，不改动用户选择的数据根的祖先目录。
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    let _ancestors = hold_directory_chain(
        path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "private directory has no parent",
            )
        })?,
        true,
    )?;
    if !path.exists() {
        create_directory_with_acl(path)?;
    }
    let directory = open_path(path, true, true)?;
    protect_handle(&directory, true)
}

/// 凭证临时文件在第一个字节写入前就带私有 DACL。
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let _ancestors = hold_directory_chain(
        path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "private file has no parent")
        })?,
        false,
    )?;
    let descriptor = PrivateDescriptor::new(false)?;
    let attributes = descriptor.attributes();
    let encoded = wide(path)?;
    let handle = unsafe {
        CreateFileW(
            encoded.as_ptr(),
            windows_sys::Win32::Foundation::GENERIC_READ
                | windows_sys::Win32::Foundation::GENERIC_WRITE
                | WRITE_DAC,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    protect_handle(&file, false)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Authorization::GetSecurityInfo;
    use windows_sys::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, CONTAINER_INHERIT_ACE, OBJECT_INHERIT_ACE,
        SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

    fn temporary(tag: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lingxi-win-acl-{tag}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn assert_private(file: &File, directory: bool) {
        let mut acl = std::ptr::null_mut();
        let mut security: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut acl,
                std::ptr::null_mut(),
                &mut security,
            )
        };
        assert_eq!(status, 0);
        assert!(!acl.is_null(), "NULL DACL exposes every local user");
        let mut info = ACL_SIZE_INFORMATION::default();
        assert_ne!(
            unsafe {
                GetAclInformation(
                    acl,
                    (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
            },
            0
        );
        assert_eq!(
            info.AceCount, 2,
            "only current user and SYSTEM may have ACEs"
        );
        let mut actual_present = 0;
        let mut actual_defaulted = 0;
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(
                    security,
                    &mut actual_present,
                    &mut acl,
                    &mut actual_defaulted,
                )
            },
            0
        );
        assert_eq!(actual_present, 1);
        let mut actual_sids = Vec::new();
        for index in 0..2 {
            let mut raw = std::ptr::null_mut();
            assert_ne!(unsafe { GetAce(acl, index, &mut raw) }, 0);
            let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
            assert_eq!(ace.Header.AceType, 0, "only allow ACEs are expected");
            assert_eq!(ace.Mask, FILE_ALL_ACCESS);
            let inheritance = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8;
            assert_eq!(ace.Header.AceFlags, if directory { inheritance } else { 0 });
            let mut sid_ptr: *mut u16 = std::ptr::null_mut();
            let sid = (&ace.SidStart as *const u32).cast_mut().cast();
            assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut sid_ptr) }, 0);
            let mut len = 0;
            while unsafe { *sid_ptr.add(len) } != 0 {
                len += 1;
            }
            actual_sids.push(
                String::from_utf16(unsafe { std::slice::from_raw_parts(sid_ptr, len) }).unwrap(),
            );
            unsafe { LocalFree(sid_ptr.cast()) };
        }
        actual_sids.sort();
        let mut expected_sids = vec![current_user_sid().unwrap(), "S-1-5-18".to_string()];
        expected_sids.sort();
        assert_eq!(actual_sids, expected_sids);
        let mut control = 0;
        let mut revision = 0;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(security, &mut control, &mut revision) },
            0
        );
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "inherited broad ACEs must stay blocked"
        );
        unsafe { LocalFree(security.cast()) };
    }

    #[test]
    fn creates_and_tightens_private_objects_before_content() {
        let directory = temporary("creation");
        ensure_private_directory(&directory).unwrap();
        let dir_handle = open_for_acl(&directory, true).unwrap();
        assert_private(&dir_handle, true);
        let file_path = directory.join("credential.json");
        let file = create_private_file(&file_path).unwrap();
        assert_private(&file, false);
        drop(file);
        // 故意用宽松 DACL 模拟旧安装；升级时必须在原句柄上收紧。
        let broad = PrivateDescriptor::parse("D:(A;;FA;;;WD)").unwrap();
        let old = open_for_acl(&file_path, false).unwrap();
        let status = unsafe {
            SetSecurityInfo(
                old.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                broad.dacl().unwrap(),
                std::ptr::null(),
            )
        };
        assert_eq!(status, 0);
        drop(old);
        ensure_private_file(&file_path).unwrap();
        assert_private(&open_for_acl(&file_path, false).unwrap(), false);
        drop(dir_handle);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_hardlinks_and_wrong_types_without_writing() {
        let directory = temporary("reject");
        ensure_private_directory(&directory).unwrap();
        let file_path = directory.join("record.json");
        drop(create_private_file(&file_path).unwrap());
        let alias = directory.join("other.json");
        std::fs::hard_link(&file_path, &alias).unwrap();
        assert!(ensure_private_file(&file_path).is_err());
        assert!(ensure_private_directory(&file_path).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn existing_external_directory_is_rejected_without_acl_mutation() {
        let directory = temporary("external");
        ensure_private_directory(&directory).unwrap();
        let file = open_for_acl(&directory, true).unwrap();
        let broad = PrivateDescriptor::parse("D:P(A;OICI;FA;;;WD)").unwrap();
        assert_eq!(
            unsafe {
                SetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    broad.dacl().unwrap(),
                    std::ptr::null(),
                )
            },
            0
        );
        drop(file);
        assert!(require_private_directory(&directory).is_err());
        // 验证动作不得悄悄收紧用户选择的既有目录。
        let unchanged = open_for_acl(&directory, true).unwrap();
        assert!(verify_private_handle(&unchanged, true).is_err());
        drop(unchanged);
        ensure_private_directory(&directory).unwrap();
        drop(require_private_directory(&directory).unwrap());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
