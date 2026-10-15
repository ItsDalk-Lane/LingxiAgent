# Windows 原生只读发现模块交付

仅实现新模块，不修改 Main、parent discovery_tools、旧 filetools、门禁、原断言或登记；无提交推送。主源码 `windows_readonly.rs`，SHA256 `2875992c321fb27cda19db9a065ab6dcebccab0f25dbcf68885701fb3198112d`。w11 负责精确父模块接线与当前 host 的 Run 集成验证。

## 真实实现和依据

根只接受盘符绝对路径或 UNC 文件共享，统一长路径形式后只打开根。随后所有目录和条目都通过 NtOpenFile 的 OBJECT_ATTRIBUTES.RootDirectory，以单组件名字相对已经持有的父 File 打开。不存在 canonicalize 前后两次、拼子路径 File::open 或 std read_dir 的回退。官方 [NtOpenFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntopenfile) 和 [OBJECT_ATTRIBUTES.RootDirectory](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes) 明确支持文件系统目录句柄相对路径；用户态不设置 OBJ_KERNEL_HANDLE。

每个祖先 File 都保留到 DirFd 释放，share仅READ、不共享DELETE/WRITE；内容 File 也用同样分享限制。已有冲突写/删除句柄会使新打开失败，后来的删除/重命名/写入也被限制；不是先路径检查再相信已变化路径。这个分享边界来自官方 [CreateFile分享规则](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)。根和中间祖先只请求属性/遍历权限，最终目录额外请求列表权限；没有备份权限提升、没有读失败后的提权重试。相对打开不设FILE_OPEN_FOR_BACKUP_INTENT。

所有最终打开用 FILE_OPEN_REPARSE_POINT；同一真实句柄检查reparse/目录类型，内容只接受普通disk file；中间目录发现任何reparse显式拒绝。元数据可返回kind=symlink供父层跳过所有reparse（包括junction），不会打开目标内容。未请求写权限、创建、截断或删除。规则依据 [原生文件打开选项](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-zwcreatefile)；只读目录不应因类型检查去读越权内容。

目录名使用 GetFileInformationByHandleEx 的 FileFullDirectoryRestartInfo / FileFullDirectoryInfo，直接枚举被持有的同一目录句柄。固定32KiB、8字节对齐缓冲，每条检查长度/链偏移；每条和每次系统调用前都检查取消，100000条以及有界批次，独立互斥游标，每次从头开始。官方 [FILE_FULL_DIR_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_full_dir_info) 与 [FILE_INFO_BY_HANDLE_CLASS](https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ne-minwinbase-file_info_by_handle_class) 给出布局和继续/重启语义。

身份使用 GetFileInformationByHandleEx(FileIdInfo) 的完整volume u64 + FileId u128；不能用64位FileIndex在ReFS上假装无碰撞。微软 [FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info) 是实际句柄身份依据。元数据大小组合完整高低32位，FILETIME转Unix毫秒及秒内纳秒均检查范围；mode保存实际Windows属性位，不能当Unix权限掩码。

## 父模块接口

- `DirFd::open(&Path) -> io::Result<Self>`：调用方给已经ResourceAccess授权的canonical绝对路径。
- `directory_identity() -> io::Result<(u64,u128)>`，`same_directory(&Path) -> io::Result<bool>`。
- `entry_names(cancelled: impl Fn() -> bool) -> io::Result<Vec<OsString>>`。
- `entry_metadata(&OsStr) -> io::Result<EntryMetadata>`；kind/size/modified_ms/device/inode/mode/modified_nsec字段与Unix对应，但inode统一u128。
- `open_attachment_nofollow(&OsStr) -> Option<io::Result<File>>`：仅NotFound返回None，其它权限/共享/类型/重解析错误保留。
- `file_identity(&File) -> io::Result<(u64,u128)>`：替代父read直接Unix MetadataExt::dev/ino；Unix helper把原u64 inode无损转u128。

w11已确认接受上述身份的小类型调整；父Engine的directory集合、Frame.expected及对应元数据inode都同步(u64,u128)，没有哈希/截断身份。原Unix文件工具模块不必为了此新增Windows读模块作宽重构。Windows生产入口可以真实Available，资源权限、工具取消/大小预算、发现消费者由现有父Engine继续执行，不能把“未实机验证”继续包装成全平台Disabled。

重要生命周期：父read必须让DirFd一直活到File内容读完、源元数据复核和后验授权结束；标准File不能携带祖先保护，不能先drop DirFd再读File。w11确认当前read的局部dir覆盖这个全过程。输入文件max预算仍由父Engine的64KiB分块/取消和MAX_FILE处理；元数据size是无损值，绝不自行读整文件绕过预算。

绝对盘符/普通UNC和verbatim UNC/长路径实际支持。设备命名空间、盘符相对路径、命名管道共享、ADS冒号、父项/分隔符/NUL、Win32含糊尾点尾空格、DOS保留设备名都明确拒绝；单组件上限255 UTF-16、路径32766 UTF-16、祖先1024层。无额外依赖。只依赖Windows8+原生API（产品Windows10+范围覆盖）；文件系统或SMB不提供所需句柄身份/目录信息时保留真实错误，不假造成功。

## 验证与限制

固定rustc1.98.1。只安装该已有工具链的x86_64-pc-windows-gnu标准库，没有重装工具链、构建巨型service或复制大target。库与9项Windows测试全部真实Windows cfg metadata编译 -D warnings通过；独立无依赖小Cargo检查 clippy --locked --offline --target x86_64-pc-windows-gnu --lib --tests -- -D warnings通过，日志 `windows-clippy.log`；fmt通过。初次误用tmp默认rustc找不到目标，改为明确+1.98.1；一轮私有函数形参调整未改完导致编译失败已修正；初版Clippy提出整除/分片惯用写法后等价修正，没有加allow或改断言。

9项原生测试代码：真实目录/隐藏项/UTF-8文件与同一完整身份；祖先/目录/内容句柄阻止改名和写入并RAII释放；已有写句柄时拒绝读后恢复；实际文件/目录symlink不读外部内容；取消与非法设备/流/父项；真实长路径与稀疏大文件64位长度；解析界限/取消/损坏链；真实icacls拒读与恢复；UNC/长UNC解析同根。symlink测试要求Windows CI的Developer Mode或创建链接权限，缺失会明确失败；大文件先用FSCTL_SET_SPARSE，避免实际占用4GiB，测试host需NTFS；ACL仅修改fixture自己的临时文件并在断言前恢复。

当前host是macOS：Windows实机行为、Windows链接、九项运行、远程UNC实机、WindowsARM64目标和最终service集成都未执行，不能写成PASS。取消在每次原生调用前/返回记录边界可见；不声称能够抢占单次已经阻塞的内核/SMB系统调用，父超时/TaskSupervisor仍须按实际边界结算。测试与实现是供真实Windows CI执行的可运行代码，不是固定成功替身。
