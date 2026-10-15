# R3 native ls/find/grep 独立复核

结论：**macOS arm64 本次真实行为范围 PASS；完整 R3 NEEDS_FIX**。发现 Windows 绝对路径匹配确定缺口。Windows 后端已经实现且接为 Available，不能记为 UNIMPLEMENTED；Windows/Linux 实机结果仍 NOT_RUN，不能据本机通过签跨平台完成或 R06_ACCEPTED/R07_READY。

本轮只读审查冻结源码，执行给定冻结当前 service 与正式 Rust 测试父；未修改 Main、作者源码、原断言、门禁或登记，未提交。未创建 Cargo target，未执行新 service 编译；复用现有 target 内既有可执行文件，合成 home/TMPDIR/product/HTTP provider，未读取真实用户业务数据。R1/R2 报告和原始失败未覆盖。

## 冻结候选及实际执行

- patch `/tmp/lingxi-native-discovery-r3.patch` SHA256 `7c8a73d037fb502a771ba2feee4e03a375f72287bc4684405ecca02aca94a78b`。
- service `/tmp/lingxi-ex01-target/debug/lingxi-service` SHA256 `036f54c562f3debf15d5e0c3abade337d9859dd07349cb7258c2bcb312fc29c5`。
- 作者树 `/tmp/lingxi-archive-restore-codex`；production+test 十文件逐个重算，与冻结 source-proof 全部一致：`/tmp/native-discovery-r3-independent-source-check.json`。
- `discovery_tools.rs` SHA `43cf4b9e732ff252451bf14c05bff2c7bffc7aab5e23e315286f1a2f7996eadc`，Windows 模块 `6178b954d4b83e983e8ff81503f704ad293905d2db1b92b6662569e22864e909`。

独立执行正式父（环境均为新建合成目录）：

```sh
cd /tmp/lingxi-archive-restore-codex
env HOME=/tmp/native-discovery-r3-review-runtime/home TMPDIR=/tmp/native-discovery-r3-review-runtime/tmp LINGXI_HOME=/tmp/native-discovery-r3-review-runtime/home PYTHONDONTWRITEBYTECODE=1 CARGO_INCREMENTAL=0 /tmp/lingxi-ex01-target/debug/deps/r06_native_discovery_current_source-7c06049c2cd8c29c --exact macos_native_discovery_current_binary_real_run_and_boundaries --nocapture
```

实际退出 0，1 passed / 0 failed / 0 ignored，37.65s；原40 + 原反斜线/Unicode/预算6 + 深400/下一Run/真实取消4，共50子项全PASS。日志 `/tmp/native-discovery-r3-independent-parent.log`；原始证据 `/private/tmp/lingxi-native-current-12558-1792023556188782000`；正式收据 `parent-receipt.json` 明确 binary SHA 与 `PASS_MAC_ONLY`。深400实际27.182s，精确路径保留，下一Run继续接受；断开真实请求使Run cancelled，已有物理调用未假造 succeeded。

## P2：Windows 普通绝对 glob 被扩展路径前缀破坏

位置：`rust/crates/lingxi-service/src/discovery_tools.rs:483–493,580–584,706–712,764–768`。

`ResourceAccess.authorize` 经 `std::fs::canonicalize` 产生 Windows extended-length 绝对路径。`path_text` 仅把反斜线换成 `/`；`glob_matches` 对绝对模式拿此文本直接与有首尾锚点的 glob 比较。于是合法模式 `C:/root/*.txt` 的目标从 `\\?\C:\root\a.txt` 变成 `//?/C:/root/a.txt`，并不匹配。UNC 同理：`//server/share/root/*.txt` 对 `//?/UNC/server/share/root/a.txt` 不匹配。find 返回空；grep 的绝对 `glob` 过滤复用同一分支也漏掉文件。相对返回路径不受此前缀问题影响，不能误报成所有路径损坏。

独立表达式证据：`/tmp/native-discovery-r3-windows-absolute-glob-probe.rs` 从冻结文件逐字提取整个 `glob` 函数，在宿主用实际 regex 库编译执行。输出 `/tmp/native-discovery-r3-windows-absolute-glob-probe.log`：驱动器与UNC两组都 `actualMatch=false expectedControlMatch=true`。正对照是同一实际 glob 正确命中普通绝对目标文本；负控仅改为生产会产生的扩展路径替换文本。使用绝对 `/Users/study_superior/.cargo/bin/rustc`，版本1.98.1，无Cargo新target。源提取证明 `/tmp/native-discovery-r3-windows-absolute-glob-probe-source.json`。

这属于**源码及真实宿主表达式执行确认**，没有伪称 Windows 运行。官方语义依据：[Rust canonicalize](https://doc.rust-lang.org/std/fs/fn.canonicalize.html) 明确 Windows 转为 extended-length syntax；[Rust Prefix](https://doc.rust-lang.org/std/path/enum.Prefix.html) 区分普通/VerbatimDisk/VerbatimUNC。

最小修正建议：Windows 的模式与目标使用同一种、保留驱动器/UNC身份的匹配表示；兼顾普通绝对模式、扩展绝对模式、相对模式及 glob 的反斜线转义。补驱动器与UNC普通/扩展前缀等价正对照，并覆盖 find 与 grep glob；不要改弱原断言。当前无需改 Unix 反斜线文件名行为。

## 独立实际边界反例

我另外运行 `/tmp/native-discovery-r3-frame-probes.py`，新输出 `/tmp/native-discovery-r3-independent-frames`，日志 `/tmp/native-discovery-r3-frame-probes.log`。三项全PASS，binary SHA同上。保留原R2脚本/输出不改写。

1. `directory-replaced`：等待真实调用账本 started，lsof观察220深子树实际打开目录后替换祖先，工具明确失败，拒绝伪造新目录内容，无 succeeded 收据。
2. `ancestor-moved-outside-and-link`：同样等物理句柄后移走祖先，再放通向授权外同深树的符号链接，明确拒绝，不披露 outside secret。
3. `actual-deep-frame-cancel-lease-and-next-run`：深层物理扫描时并发同会话409；断开真实请求后 cancelled，无成功工具完成事件，物理占用释放后下一真实Run成功；原文件保持，session_files/file_history_snapshots零。

这是实际HTTP、真实service、真实Run/Gateway/存储/文件系统检查。外部模型HTTP仅为协议替身；未mock服务、业务、存储或生产文件操作。

## 三叶原条款逐句结论

原始条款来源 `scripts/rust-tauri/r06_original_leaf_bindings.json`，未改。

| 原叶/原句 | macOS arm64实际证据与结论 | 尚缺范围 |
|---|---|---|
| B97D0D4296FA「列表准确」 | 真实ls目录、隐藏项、类型/大小/时间、分页/默认路径、Unix实际文件名正对照，PASS | 其他平台实机 |
| B97D「没有写入」；「返回目录项与元数据，不改文件。」 | 原40及6/depth整套文件摘要/产物登记对照，PASS | 其他平台实机 |
| B97D「目录不存在、越权/符号链接逃逸时错误。」 | 真实missing/越权/链接、实际权限错误、祖先替换反例，PASS | Windows ACL/reparse/share实机、Linux实机 |
| F88B0A969094「命中路径准确」 | 真实递归glob、smart case、反斜线原输入、深400精确命中、分页，PASS | Windows普通绝对glob NEEDS_FIX；其他平台实机 |
| F88B「无写入」；「返回路径列表，不写文件。」 | 全套真实文件/产物登记对照，PASS | 其他平台实机 |
| F88B「越权路径或扫描错误明确失败；无匹配返回空列表。」 | 真空集、权限/链接/目录替换失败、预算明确错误/真实取消，PASS | Windows完整service运行、其他平台实机；不得把上述错误空集当正确无匹配 |
| EC77C2B04726「匹配位置正确」 | 真regex/literal、中文/astral Unicode/BOM/CRLF行列与字节位置/源摘要、context/offset/count，PASS | Windows绝对glob过滤 NEEDS_FIX；其他平台实机 |
| EC77「无写入」；「返回匹配行/位置，不修改文件。」 | 全套摘要/零伪文件登记、独立取消原文件保持，PASS | 其他平台实机 |
| EC77「无匹配为空；无效正则/越权路径明确失败。」 | 真空集、invalid regex、越权/权限、文件/行预算/FIFO/取消错误，PASS | Windows/Linux实机；绝对过滤的漏命中不能当正确空集 |

## Windows FFI、边界与正式父静态复核

没有发现另一个能够确认的FFI布局或授权绕过缺陷。UNICODE_STRING字节长度、OBJECT_ATTRIBUTES RootDirectory、IO_STATUS_BLOCK指针宽度联合体、FileFullDirectoryInfo名称偏移68/长度偏移60/链对齐8、NtOpenFile同步选项与SYNCHRONIZE接线一致；volume+完整u128 file ID传入共享引擎，Unix ino升宽而未截断。目录枚举固定32KiB缓冲，有记录/批次上限、逐批/逐条取消和解析边界检查。

目录从可信盘符/UNC根开始，只允许普通组件；逐组件RootDirectory相对打开，OPEN_REPARSE_POINT检测，拒绝device/ADS/非法名称，保留根和祖先句柄，share只读以拒绝写/删除竞争；文件内容来自同一打开句柄，读前/后身份/元数据和父目录/授权复验接通。共享层仍允许在授权后解析符号链接目标，README“父层跳过所有reparse”的表述不精确，但本轮没有将此自动包装成授权绕过。Windows已经阻塞在内核内的一次I/O并没有被证明可即时打断，不能把循环取消检查扩大为任意网络盘阻塞可抢占。

正式父确实使用 `env!(CARGO_BIN_EXE_lingxi-service)` 指定的当前service；执行checked-in脚本，要求子进程退出成功、固定原40/6/4数量且全部PASS、零写入/假产物和对应证据非空；把实际二进制SHA与脚本SHA写收据。没有把作者文本报告当通过，也未借用R2二进制。独立只读核对了父与十文件源摘要；本审阅未fresh编译service，所以构建来源仍依赖冻结当前构建交付及其源证明，运行本身由本审阅重新执行。

Windows模块9个cfg测试仍存在，作者交付的 GNU metadata/-Dwarnings/strictclippy属于编译检查，本轮没有重新执行；它们没有在Windows运行，也不等于整个service完成交叉编译/链接。官方布局依据包括 [NtOpenFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntopenfile)、[FILE_FULL_DIR_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_full_dir_info)、[FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)。

## 范围与原失败保留

- 本机 macOS arm64：本次50+3真实检查范围PASS。
- macOS x64、Linux x64：实现存在，实机NOT_RUN。
- Windows x64：实现存在且生产Available；绝对glob需修；实机/链接/整个service目标构建NOT_RUN。
- COMMON-05-L0057等原四平台要求仍有效。没有机器不是删除平台或判N/A的依据；本机结果不能封三叶跨平台完成。
- 未执行真实供应商、Electron/Tauri窗口、正式打包、完整门禁；不签R06_ACCEPTED/R07_READY。
- 原R1 `/tmp/native-discovery-independent-review.md`、原反斜线 `/tmp/native-discovery-independent-probes`、原400崩溃 `/tmp/native-discovery-independent-depth400`，及R2报告 `/tmp/native-discovery-r2-independent-review.md` 均保留。

机器结论：`/tmp/native-discovery-r3-independent-confirmation.json`。
