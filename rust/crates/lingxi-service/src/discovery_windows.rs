//! Windows 真实只读目录句柄：根后逐组件相对打开，同一目录句柄直接枚举。
//! 调用方必须先完成 ResourceAccess 授权，并让 DirFd 活到内容读取及后验授权结束。
#![cfg(windows)]
use std::{
    ffi::{c_void, OsStr, OsString},
    fs::{File, OpenOptions},
    io,
    mem::{size_of, MaybeUninit},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, RawHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    ptr,
    sync::Mutex,
};

const FILE_READ_DATA: u32 = 0x0001;
const FILE_LIST_DIRECTORY: u32 = 0x0001;
const FILE_TRAVERSE: u32 = 0x0020;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 0x0001;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_DIRECTORY_FILE: u32 = 0x0001;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0040;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0020;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
const FILE_TYPE_DISK: u32 = 1;
const FILE_FULL_DIRECTORY_INFO: i32 = 14;
const FILE_FULL_DIRECTORY_RESTART_INFO: i32 = 15;
const FILE_ID_INFO: i32 = 18;
const ERROR_NO_MORE_FILES: i32 = 18;
const MAX_ENTRIES: usize = 100_000;
const MAX_COMPONENTS: usize = 1024;
const MAX_UTF16_PATH: usize = 32_766;
const SYMLINK_SWAP_MARK: &str = "SYMLINK-OR-REPARSE-WHERE-A-REAL-FILE-WAS-AUTHORIZED";

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: RawHandle,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}
#[repr(C)]
struct IoStatusBlock {
    status_or_pointer: usize,
    information: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
struct ByHandleInformation {
    attributes: u32,
    creation: FileTime,
    access: FileTime,
    write: FileTime,
    volume: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}
#[repr(C)]
struct FileIdInformation {
    volume: u64,
    id: [u8; 16],
}
const _: () = {
    assert!(size_of::<FileIdInformation>() == 24);
    assert!(size_of::<ByHandleInformation>() == 52);
    assert!(std::mem::offset_of!(ObjectAttributes, root_directory) == size_of::<usize>());
    assert!(size_of::<IoStatusBlock>() == 2 * size_of::<usize>());
};
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenFile(
        handle: *mut RawHandle,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatusBlock,
        share: u32,
        options: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(handle: RawHandle, information: *mut ByHandleInformation) -> i32;
    fn GetFileInformationByHandleEx(
        handle: RawHandle,
        class: i32,
        information: *mut c_void,
        bytes: u32,
    ) -> i32;
    fn GetFileType(handle: RawHandle) -> u32;
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "native discovery cancelled")
}
fn reparse() -> io::Error {
    io::Error::other(SYMLINK_SWAP_MARK)
}

/// 不允许流名、设备别名、分隔符或隐式路径规范化把一个条目变成另一对象。
fn validate_component(name: &OsStr) -> io::Result<()> {
    let units: Vec<u16> = name.encode_wide().collect();
    if units.is_empty()
        || units.len() > 255
        || units == [46]
        || units == [46, 46]
        || units.last().is_some_and(|c| *c == 46 || *c == 32)
        || units
            .iter()
            .any(|c| *c < 32 || matches!(*c, 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
    {
        return Err(invalid(
            "native Windows entry must be one unambiguous file name",
        ));
    }
    // 原生相对打开本身不解析DOS设备；这里仍显式拒绝易混淆的保留名。
    let stem: Vec<u16> = units
        .iter()
        .copied()
        .take_while(|c| *c != 46)
        .map(|c| if (97..=122).contains(&c) { c - 32 } else { c })
        .collect();
    let ascii = String::from_utf16(&stem).unwrap_or_default();
    if matches!(
        ascii.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (stem.len() == 4
        && matches!(&stem[..3], [67, 79, 77] | [76, 80, 84])
        && (matches!(stem[3], 49..=57 | 0x00B9 | 0x00B2 | 0x00B3)))
    {
        return Err(invalid("native Windows device names are not file entries"));
    }
    Ok(())
}

/// 只接受盘符绝对路径和UNC文件共享；根转换为长路径形式，后续不再解析盘符路径。
fn root_and_names(path: &Path) -> io::Result<(PathBuf, Vec<OsString>)> {
    if path.as_os_str().encode_wide().count() > MAX_UTF16_PATH {
        return Err(invalid("native Windows path exceeds UTF-16 path bound"));
    }
    let mut components = path.components();
    let prefix = match components.next() {
        Some(Component::Prefix(prefix)) => prefix,
        _ => return Err(invalid("native Windows path must be absolute")),
    };
    let root = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
            PathBuf::from(format!("\\\\?\\{}:\\", char::from(drive)))
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            validate_component(server)?;
            validate_component(share)?;
            if share.to_string_lossy().eq_ignore_ascii_case("pipe") {
                return Err(invalid("named pipe namespace is not a file share"));
            }
            let mut root = OsString::from("\\\\?\\UNC\\");
            root.push(server);
            root.push("\\");
            root.push(share);
            root.push("\\");
            PathBuf::from(root)
        }
        _ => return Err(invalid("native Windows device namespaces are not accepted")),
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(invalid("drive-relative Windows paths are not accepted"));
    }
    let mut names = Vec::new();
    for component in components {
        match component {
            Component::Normal(name) => {
                validate_component(name)?;
                names.push(name.to_os_string());
            }
            _ => {
                return Err(invalid(
                    "native Windows path must not contain relative components",
                ));
            }
        }
        if names.len() > MAX_COMPONENTS {
            return Err(invalid("native Windows path has too many components"));
        }
    }
    Ok((root, names))
}

fn information(file: &File) -> io::Result<ByHandleInformation> {
    // 安全依据：File持有有效句柄；输出缓冲与官方C结构同布局，成功后才读取。
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native discovery only accepts disk file handles",
        ));
    }
    let mut value = MaybeUninit::<ByHandleInformation>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), value.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { value.assume_init() })
}
/// 采用完整128位标识，不把ReFS身份截断成64位，也不由路径或时间猜身份。
pub fn file_identity(file: &File) -> io::Result<(u64, u128)> {
    let _ = information(file)?;
    let mut value = MaybeUninit::<FileIdInformation>::uninit();
    // 安全依据：原生输出类型24字节/8字节对齐，FILE_ID_INFO类只写此输出缓冲。
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FILE_ID_INFO,
            value.as_mut_ptr().cast(),
            size_of::<FileIdInformation>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let value = unsafe { value.assume_init() };
    Ok((value.volume, u128::from_le_bytes(value.id)))
}
fn assert_directory(file: &File) -> io::Result<()> {
    let info = information(file)?;
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(reparse());
    }
    if info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "native parent is not a directory",
        ));
    }
    Ok(())
}

/// 所有子项只通过当前句柄相对打开；没有路径重开或canonicalize前后比较的回退。
fn open_relative(
    parent: &File,
    name: &OsStr,
    content: bool,
    directory: bool,
    enumerate: bool,
) -> io::Result<File> {
    validate_component(name)?;
    let mut wide: Vec<u16> = name.encode_wide().collect();
    let length =
        u16::try_from(wide.len() * 2).map_err(|_| invalid("native entry name is too long"))?;
    let mut unicode = UnicodeString {
        length,
        maximum_length: length,
        buffer: wide.as_mut_ptr(),
    };
    let mut attrs = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut unicode,
        // 已授权canonical路径/枚举名使用实际大小写，不强制折叠区分大小写的目录。
        attributes: 0,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut status = IoStatusBlock {
        status_or_pointer: 0,
        information: 0,
    };
    let mut raw: RawHandle = ptr::null_mut();
    let access = FILE_READ_ATTRIBUTES
        | SYNCHRONIZE
        | if content { FILE_READ_DATA } else { 0 }
        | if directory { FILE_TRAVERSE } else { 0 }
        | if enumerate { FILE_LIST_DIRECTORY } else { 0 };
    let options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if directory {
            FILE_DIRECTORY_FILE
        } else if content {
            FILE_NON_DIRECTORY_FILE
        } else {
            0
        };
    // 安全依据：所有指针在同步调用期间有效；相对名只有一段，RootDirectory为存活File。
    // 不共享DELETE/WRITE，已有冲突句柄使本次失败，新rename/write句柄同样被拒绝。
    let result = unsafe {
        NtOpenFile(
            &mut raw,
            access,
            &mut attrs,
            &mut status,
            FILE_SHARE_READ,
            options,
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    if raw.is_null() || raw as isize == -1 {
        return Err(io::Error::other(
            "NtOpenFile returned an invalid success handle",
        ));
    }
    // 安全依据：NT成功返回的新句柄由File唯一接管，后续错误同样通过RAII关闭。
    let file = unsafe { File::from_raw_handle(raw) };
    let info = information(&file)?;
    if content && (info.attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)) != 0
    {
        return Err(reparse());
    }
    if directory {
        assert_directory(&file)?;
    }
    Ok(file)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMetadata {
    pub kind: &'static str,
    pub size: u64,
    pub modified_ms: i64,
    pub device: u64,
    pub inode: u128,
    /// Windows真实属性位；不能把它解释成Unix权限掩码。
    pub mode: u32,
    pub modified_nsec: i64,
}

pub struct DirFd {
    // 根到当前目录每层都保留，不允许扫描期间祖先被移动/删除。
    chain: Vec<File>,
    enumeration: Mutex<()>,
}
impl DirFd {
    pub fn open(path: &Path) -> io::Result<Self> {
        let (root, names) = root_and_names(path)?;
        let root_file = OpenOptions::new()
            .access_mode(
                FILE_READ_ATTRIBUTES
                    | SYNCHRONIZE
                    | FILE_TRAVERSE
                    | if names.is_empty() { FILE_READ_DATA } else { 0 },
            )
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(root)?;
        assert_directory(&root_file)?;
        let mut chain = vec![root_file];
        for (index, name) in names.iter().enumerate() {
            // 中间祖先只查询属性/身份；最终目录才请求枚举权限。
            let last = index + 1 == names.len();
            let child = open_relative(chain.last().unwrap(), name, false, true, last)?;
            assert_directory(&child)?;
            chain.push(child);
        }
        Ok(Self {
            chain,
            enumeration: Mutex::new(()),
        })
    }
    fn current(&self) -> &File {
        self.chain.last().unwrap()
    }
    pub fn directory_identity(&self) -> io::Result<(u64, u128)> {
        file_identity(self.current())
    }
    pub fn same_directory(&self, path: &Path) -> io::Result<bool> {
        let other = Self::open(path)?;
        Ok(self.directory_identity()? == other.directory_identity()?)
    }
    pub fn entry_metadata(&self, name: &OsStr) -> io::Result<EntryMetadata> {
        let file = open_relative(self.current(), name, false, false, false)?;
        let info = information(&file)?;
        let (device, inode) = file_identity(&file)?;
        let ticks = ((info.write.high as u64) << 32) | u64::from(info.write.low);
        let unix_ticks = i128::from(ticks) - 116_444_736_000_000_000_i128;
        let modified_ms = i64::try_from(unix_ticks.div_euclid(10_000))
            .map_err(|_| io::Error::other("native Windows timestamp overflow"))?;
        let modified_nsec = (unix_ticks.rem_euclid(10_000_000) * 100) as i64;
        Ok(EntryMetadata {
            kind: if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                "symlink"
            } else if info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                "directory"
            } else {
                "file"
            },
            size: (u64::from(info.size_high) << 32) | u64::from(info.size_low),
            modified_ms,
            device,
            inode,
            mode: info.attributes,
            modified_nsec,
        })
    }
    pub fn open_attachment_nofollow(&self, name: &OsStr) -> Option<io::Result<File>> {
        match open_relative(self.current(), name, true, false, false) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            result => Some(result),
        }
    }
    pub fn entry_names(&self, is_cancelled: impl Fn() -> bool) -> io::Result<Vec<OsString>> {
        if is_cancelled() {
            return Err(cancelled());
        }
        let _enumeration = self
            .enumeration
            .lock()
            .map_err(|_| io::Error::other("native enumeration lock poisoned"))?;
        let mut names = Vec::new();
        let mut first = true;
        let mut batches = 0_usize;
        loop {
            batches += 1;
            if batches > MAX_ENTRIES + 2 {
                return Err(io::Error::other(
                    "native directory exceeds enumeration call bound",
                ));
            }
            if is_cancelled() {
                return Err(cancelled());
            }
            // FILE_FULL_DIR_INFO要求8字节对齐；每次只申请32KiB，永不随目录大小无限增长。
            let mut aligned = [0_u64; 4096];
            let bytes = unsafe {
                std::slice::from_raw_parts_mut(
                    aligned.as_mut_ptr().cast::<u8>(),
                    size_of_val(&aligned),
                )
            };
            let class = if first {
                FILE_FULL_DIRECTORY_RESTART_INFO
            } else {
                FILE_FULL_DIRECTORY_INFO
            };
            first = false;
            if unsafe {
                GetFileInformationByHandleEx(
                    self.current().as_raw_handle(),
                    class,
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                )
            } == 0
            {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES) {
                    break;
                }
                return Err(error);
            }
            append_directory_batch(bytes, &mut names, &is_cancelled)?;
        }
        Ok(names)
    }
}
fn append_directory_batch(
    bytes: &[u8],
    names: &mut Vec<OsString>,
    is_cancelled: &impl Fn() -> bool,
) -> io::Result<()> {
    const NAME_OFFSET: usize = 68;
    let mut offset = 0_usize;
    loop {
        if is_cancelled() {
            return Err(cancelled());
        }
        let header = bytes
            .get(
                offset
                    ..offset
                        .checked_add(NAME_OFFSET)
                        .ok_or_else(|| io::Error::other("invalid directory offset"))?,
            )
            .ok_or_else(|| io::Error::other("short native directory record"))?;
        let next = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
        let length = u32::from_le_bytes(header[60..64].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(NAME_OFFSET)
            .and_then(|n| n.checked_add(length))
            .ok_or_else(|| io::Error::other("directory name length overflow"))?;
        if length == 0 || !length.is_multiple_of(2) || length > 510 {
            return Err(io::Error::other("invalid native directory name size"));
        }
        let raw = bytes
            .get(offset + NAME_OFFSET..end)
            .ok_or_else(|| io::Error::other("native directory name exceeds buffer"))?;
        let wide: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let name = OsString::from_wide(&wide);
        if name != OsStr::new(".") && name != OsStr::new("..") {
            validate_component(&name)?;
            if names.len() == MAX_ENTRIES {
                return Err(io::Error::other(
                    "native directory exceeds 100000 entry bound",
                ));
            }
            names.push(name);
        }
        if next == 0 {
            return Ok(());
        }
        if !next.is_multiple_of(8) || next < NAME_OFFSET + length {
            return Err(io::Error::other("invalid native directory next offset"));
        }
        offset = offset
            .checked_add(next)
            .ok_or_else(|| io::Error::other("native directory offset overflow"))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "lingxi-native-win-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(std::fs::canonicalize(root).unwrap())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn real_names_metadata_and_content_share_complete_native_identity() {
        let root = Fixture::new();
        std::fs::write(root.0.join(".hidden"), "真实UTF-8文字").unwrap();
        std::fs::create_dir(root.0.join("child")).unwrap();
        let dir = DirFd::open(&root.0).unwrap();
        let names = dir.entry_names(|| false).unwrap();
        assert!(names.contains(&OsString::from(".hidden")));
        assert!(names.contains(&OsString::from("child")));
        assert_eq!(dir.entry_names(|| false).unwrap().len(), 2);
        let meta = dir.entry_metadata(OsStr::new(".hidden")).unwrap();
        assert_eq!(meta.kind, "file");
        assert_eq!(meta.size, "真实UTF-8文字".len() as u64);
        let mut file = dir
            .open_attachment_nofollow(OsStr::new(".hidden"))
            .unwrap()
            .unwrap();
        assert_eq!(file_identity(&file).unwrap(), (meta.device, meta.inode));
        let mut actual = String::new();
        file.read_to_string(&mut actual).unwrap();
        assert_eq!(actual, "真实UTF-8文字");
        assert_eq!(
            dir.entry_metadata(OsStr::new("child")).unwrap().kind,
            "directory"
        );
        assert!(dir.same_directory(&root.0).unwrap());
        assert!(dir
            .open_attachment_nofollow(OsStr::new("missing"))
            .is_none());
    }
    #[test]
    fn pinned_all_ancestors_and_final_file_reject_rename_and_writers_then_release() {
        let root = Fixture::new();
        let parent = root.0.join("parent");
        let child = parent.join("child");
        std::fs::create_dir_all(&child).unwrap();
        let path = child.join("input.txt");
        std::fs::write(&path, b"original").unwrap();
        let dir = DirFd::open(&child).unwrap();
        assert!(std::fs::rename(&parent, root.0.join("moved")).is_err());
        assert!(std::fs::rename(&child, parent.join("moved-child")).is_err());
        let file = dir
            .open_attachment_nofollow(OsStr::new("input.txt"))
            .unwrap()
            .unwrap();
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::rename(&path, child.join("moved.txt")).is_err());
        drop(file);
        drop(dir);
        let mut writer = OpenOptions::new().write(true).open(&path).unwrap();
        writer.write_all(b"changed!").unwrap();
        drop(writer);
        std::fs::rename(&parent, root.0.join("moved")).unwrap();
    }
    #[test]
    fn existing_writer_refuses_content_instead_of_reading_mutating_bytes() {
        let root = Fixture::new();
        let path = root.0.join("input.txt");
        std::fs::write(&path, b"original").unwrap();
        let writer = OpenOptions::new()
            .write(true)
            .share_mode(7)
            .open(&path)
            .unwrap();
        let dir = DirFd::open(&root.0).unwrap();
        assert!(dir
            .open_attachment_nofollow(OsStr::new("input.txt"))
            .unwrap()
            .is_err());
        drop(writer);
        assert!(dir
            .open_attachment_nofollow(OsStr::new("input.txt"))
            .unwrap()
            .is_ok());
    }
    #[test]
    fn actual_file_and_directory_symlinks_never_follow_outside_data() {
        let root = Fixture::new();
        let outside = Fixture::new();
        let secret = outside.0.join("secret");
        std::fs::write(&secret, b"outside-private-content").unwrap();
        // Windows CI应启用Developer Mode或创建链接权限；缺失权限明确失败，不假报已验。
        std::os::windows::fs::symlink_file(&secret, root.0.join("file-link")).unwrap();
        std::os::windows::fs::symlink_dir(&outside.0, root.0.join("dir-link")).unwrap();
        let dir = DirFd::open(&root.0).unwrap();
        assert_eq!(
            dir.entry_metadata(OsStr::new("file-link")).unwrap().kind,
            "symlink"
        );
        assert_eq!(
            dir.entry_metadata(OsStr::new("dir-link")).unwrap().kind,
            "symlink"
        );
        assert!(dir
            .open_attachment_nofollow(OsStr::new("file-link"))
            .unwrap()
            .is_err());
        assert!(DirFd::open(&root.0.join("dir-link")).is_err());
        assert_eq!(std::fs::read(&secret).unwrap(), b"outside-private-content");
    }
    #[test]
    fn cancellation_and_invalid_names_cannot_become_device_stream_or_parent_reads() {
        let root = Fixture::new();
        let dir = DirFd::open(&root.0).unwrap();
        assert_eq!(
            dir.entry_names(|| true).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a:stream",
            "NUL",
            "CON.txt",
            "COM1",
            "LPT9.log",
            "trailing.",
            "trailing ",
        ] {
            assert!(dir.entry_metadata(OsStr::new(name)).is_err(), "{name:?}");
            assert!(
                dir.open_attachment_nofollow(OsStr::new(name))
                    .unwrap()
                    .is_err(),
                "{name:?}"
            );
        }
        assert!(DirFd::open(Path::new("relative")).is_err());
        assert!(DirFd::open(Path::new("C:relative")).is_err());
        assert!(DirFd::open(Path::new("\\\\.\\PhysicalDrive0")).is_err());
        assert!(DirFd::open(Path::new("\\\\server\\pipe\\name")).is_err());
    }
    #[test]
    fn long_verbatim_paths_and_large_file_metadata_are_not_truncated() {
        let root = Fixture::new();
        let mut deep = root.0.clone();
        for index in 0..8 {
            deep.push(format!("segment-{index}-{}", "x".repeat(32)));
            std::fs::create_dir(&deep).unwrap();
        }
        let path = deep.join("large.txt");
        let file = File::create(&path).unwrap();
        // 真正稀疏文件验证64位长度，避免测试实际占用4GiB磁盘；Windows CI使用NTFS。
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn DeviceIoControl(
                handle: RawHandle,
                code: u32,
                input: *mut c_void,
                input_bytes: u32,
                output: *mut c_void,
                output_bytes: u32,
                returned: *mut u32,
                overlapped: *mut c_void,
            ) -> i32;
        }
        let mut returned = 0_u32;
        assert_ne!(
            unsafe {
                DeviceIoControl(
                    file.as_raw_handle(),
                    0x0009_00C4,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut returned,
                    ptr::null_mut(),
                )
            },
            0,
            "NTFS sparse-file setup failed: {}",
            io::Error::last_os_error()
        );
        let expected = (u64::from(u32::MAX)) + 8193;
        file.set_len(expected).unwrap();
        drop(file);
        let dir = DirFd::open(&deep).unwrap();
        assert_eq!(
            dir.entry_metadata(OsStr::new("large.txt")).unwrap().size,
            expected
        );
    }
    #[test]
    fn parser_enforces_bounded_cancel_and_rejects_corrupt_os_record() {
        fn batch(name: &str) -> Vec<u8> {
            let wide: Vec<_> = name.encode_utf16().collect();
            let mut bytes = vec![0_u8; 68 + wide.len() * 2];
            bytes[60..64].copy_from_slice(&((wide.len() * 2) as u32).to_le_bytes());
            for (index, unit) in wide.iter().enumerate() {
                bytes[68 + index * 2..70 + index * 2].copy_from_slice(&unit.to_le_bytes());
            }
            bytes
        }
        let valid = batch("actual.txt");
        let mut entries = vec![OsString::from("already"); MAX_ENTRIES];
        assert!(append_directory_batch(&valid, &mut entries, &|| false).is_err());
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(
            append_directory_batch(&valid, &mut Vec::new(), &|| true)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        let mut broken = valid;
        broken[0..4].copy_from_slice(&1_u32.to_le_bytes());
        assert!(append_directory_batch(&broken, &mut Vec::new(), &|| false).is_err());
    }
    #[test]
    fn real_acl_denial_is_returned_without_reading_content_and_is_recoverable() {
        let root = Fixture::new();
        let path = root.0.join("denied.txt");
        std::fs::write(&path, b"private-content").unwrap();
        let dir = DirFd::open(&root.0).unwrap();
        let denied = std::process::Command::new("icacls")
            .arg(&path)
            .args(["/deny", "*S-1-1-0:(R)"])
            .output()
            .unwrap();
        assert!(
            denied.status.success(),
            "icacls failed to create real test ACL"
        );
        let result = dir
            .open_attachment_nofollow(OsStr::new("denied.txt"))
            .unwrap();
        // 先恢复唯一测试文件的ACL，失败断言也不能污染清理或后续测试。
        let restored = std::process::Command::new("icacls")
            .arg(&path)
            .args(["/remove:d", "*S-1-1-0"])
            .output()
            .unwrap();
        assert!(
            restored.status.success(),
            "icacls failed to restore test ACL"
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(dir
            .open_attachment_nofollow(OsStr::new("denied.txt"))
            .unwrap()
            .is_ok());
    }
    #[test]
    fn unc_and_verbatim_unc_share_roots_are_parsed_without_dos_namespace_escape() {
        let (normal, a) = root_and_names(Path::new("\\\\server\\share\\目录\\file")).unwrap();
        let (verbatim, b) =
            root_and_names(Path::new("\\\\?\\UNC\\server\\share\\目录\\file")).unwrap();
        assert_eq!(normal, verbatim);
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
    }
}
