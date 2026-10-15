# 原生 ls / find / grep 作者交付

范围为 B97D0D4296FA、F88B0A969094、EC77C2B04726。原始 given/when/then/assertions 已完整读取并原样保存在 source-proof；没有改原断言、登记、白名单或门禁。知识 grep / EX09 不在此补丁。

精确源码基线是 a270368f96300827a252783c5debca4ec6ec436d。独立树历史 HEAD 不用于声称全树新基线：本项所需现役源码从这个固定提交逐文件同步，原字节和 SHA256 在 `/tmp/native-discovery-base/source-snapshot.json` 保存；交付只含新模块、三个精准配套文件及本项证据。旧归档/EX/头像改动与为编译同步的无关源码均不在净补丁内。

## 实现

- 新增正式注册的三个只读工具，复用当前 ResourceAccess、权限策略、资源范围、统一网关、真实 Run、工具原始完成事件和历史投影；保留 R04 原固定三工具注册函数。
- 保持现役七个常驻声明不变。当前模型通过 `mcp_describe_tool` 取得正式目录定义，再在同一 Run 用 `mcp_call` 实际调度三工具；不是直接伪造未声明模型工具调用。
- ls 返回实际条目和类型/大小/修改时间，包含隐藏文件；find 使用实际 glob，并保留现役 fd 的智能大小写（含大写的模式严格区分，纯小写模式忽略大小写）；grep 使用已有 Rust regex 库的正则或明确 literal，支持筛选、大小写、匹配行、字符列、原文件字节位置、上下文和真实分页。匹配行还带实际读取原字节的 SHA256。files 按修改时间、count 按命中行数排序，稳定处理并列。
- 所有路径逐层以不跟随链接的目录句柄打开；实际条目身份和已授权路径复核。链接逃逸、目录或文件读失败明确失败，不能吞成空列表。模型结果只含相对来源和受限错误原因，不输出宿主绝对目录。没有调用本地 rg/fd 命令。
- 同一真实任务的取消域传入阻塞读取；实际后台读取持有原 Run 的会话许可直到物理退出，不能在取消等待后提前让另一个 Run 接管。
- 只读工具不登记文件产物，也不创建文件历史快照。

## 有界行为和限制

单次实际扫描最多 100000 个条目；单文本文件最多 16MiB / 200000 行。超界、单条无法容纳结果和非法 glob/regex 均明确错误，不伪装无匹配。可分页结果在字节/行数上界处明确 truncated/nextOffset；仅字段裁短时没有假分页游标。非 UTF-8 或包含 NUL 的文件按文本搜索忽略，并明确 skippedBinaryFiles 数。遍历忽略 `.git` 与 `node_modules`，按实际目录身份去重并报告 skippedDuplicateDirectories；没有宣称完整复刻 fd/rg 的忽略文件规则。

本机实测是 macOS。Linux 复用同一明确启用的安全 FD 路径，但本机未运行 Linux；其它平台工具目录为 Disabled，执行也明确拒绝。没有声称 Windows 发现工具已经完成；若后续门禁明确要求 Windows 成功，该缺口仍需实际完成验收。

## 验证

真实当前服务二进制构建、Clippy `-D warnings`、四个改动文件格式检查均通过。唯一替身是通过真实 HTTP 供应商协议接入的外部模型端点；存储、授权、工具、文件、Run、原始历史、下一轮提示与重启均为真实产品路径。

最终场景与摘要见同目录 `native-discovery-cases.json` / `native-discovery-receipt.json`。原始运行证据、所有模型 HTTP 请求、原始完成事件、历史、日志、取消时操作系统目录句柄证据保留在收据标识的独立 `/tmp` 目录，不提交大运行资料。

检查覆盖 ls 精确条目/隐藏项/真实元数据/默认工作区/截断，find 递归 glob 与准确空结果，grep 真正正则/字面/筛选/大小写/Unicode/BOM/CRLF 行列与字节位置/原文摘要/上下文/计数/文件排序/分页/准确空结果，三工具越权、链接逃逸、缺失与真实 EACCES，非法正则/glob、FIFO 无阻塞拒绝、无文件或伪产物改动，以及重启后下一真实 Run 消费持久历史。

取消腿以真实 HTTP 前台请求进行：真实 started 收据之后，通过操作系统实际打开的扫描目录句柄确认物理扫描已开始，再确认并发提交返回忙，随后断开当前调用；最终 Run 为 cancelled，该 grep 无成功收据或完成事件。随后会话实际释放并接受下一真实 Run。目录说明先前成功收据仍真实保留，未将其误算为被取消 grep 的结果。

初次脚本直接调用未常驻声明的 ls，被真实供应商解析拒绝；已按当前目录合同改用 describe/call。另一轮取消观察最初按固定毫秒排序，误选同 Run 的 describe 行；已改为真实插入顺序并增加操作系统句柄证据。随后曾把 describe 的成功收据误算为 grep 成功；已精确按真实 grep target 检查。以上是测试修正，没有放宽原叶断言，也没有改生产取消判定。

旧服务测试编译缓存已用官方 `cargo clean --locked -p lingxi-service` 清除 17.8GiB，仅重建当前 bin 与必要检查，未删除源码或证据。交付不提交、不推送，不修改主工作目录。


## R2：独立复核反例及修正

独立发现两条失败链，原叶断言全部保留。

1. Unix 合法文件名 `odd\name.txt` 被公共路径格式化错改为 `odd/name.txt`，find/grep 输出路径实际不存在。R1冻结二进制 SHA 为 `c8927fa74199b6ccad22df4300994b6900af76f78bddbb22bd5056d172201c5e`，完整字节另存 ` /tmp/native-discovery-delivery/bin/r1-lingxi-service`。用原独立六条检查的参数化脚本真实重现 4 PASS / 2 FAIL；R2不再改写 Unix 反斜杠，修后同六条 6 PASS。包含额外 Unicode 字节定位、16MiB文件边界和20万行边界，所有原检查不变。
2. 仅401条目、400层合法目录，R1真实 Run 令线程栈耗尽，服务 SIGABRT（-6），独立原始证据在收据指向目录，1200层未能建出的路径没有计作产品反例。R2改用堆上遍历帧，每帧只有路径/身份/剩余名称，跨层不保留目录句柄；每次实际进入、处理条目与退出仍检查授权和真实目录身份，并持续响应取消。原400层探针现在返回真实 target.txt 唯一路径，实际回读正确；服务存活，下一真实 Run 成功。另一真实深树调用先由操作系统打开的深层目录句柄证明扫描实际开始，随后并发409、前台断线，Run最终cancelled、无该find成功收据，且同一服务随后正常新Run。该腿4 PASS。

R2原有40项不改断言重跑40 PASS，加独立6项和深树4项，共50个本机检查PASS，build/clippy严格检查/格式通过。只有外部HTTP模型替身，未伪造工具、Run、存储、输入或历史。

净R2相对当前Main的精确文件基线另行记录（当前Main lib含未提交ticker配套），仅叠加我方模块声明和工具注册两处，不覆盖任何ticker或knowledge工作。作者二进制基于最初a270精确所需源码快照加本项scope；未将其它作者尚未整合的配套宣称已在本二进制验证，Main实际整合由主控另行构建复验。

平台逐句状态：macOS本机上述50项为PASS；Linux实现路径存在但本机没有Linux运行环境，NOT_RUN；Windows在R2仍Disabled，即UNIMPLEMENTED，不是“仅未实测”。因此R2不能据此宣称三原叶的全部平台条款完成。Windows作者正准备独立安全句柄模块供R3接入，本报告未把未来实现算作已完成。


## R3：Windows生产入口及当前源码父测试

Windows冻结源2875992c321fb27cda19db9a065ab6dcebccab0f25dbcf68885701fb3198112d已接入。为符合本仓库edition2021的格式检查，仅作rustfmt等价排版，集成字节SHA变为6178b954d4b83e983e8ff81503f704ad293905d2db1b92b6662569e22864e909；没有变更其九项Windows原断言。模块名discovery_windows.rs，父统一使用完整(volume:u64, inode:u128)身份；Unix inode无损转u128，实际打开File通过统一file_identity核对。DirFd保留到实际内容读取、源元数据复核和后验授权全部结束。

Windows现在是实际Available生产分支，使用原生NtOpenFile相对父句柄单组件打开、GetFileInformationByHandleEx同句柄枚举、完整FileIdInfo、祖先/最终文件不共享WRITE/DELETE、reparse明确拒绝，不是std路径扫描回退。父共享正式registry/ResourceAccess/gateway/Run/预算和真实取消域。反斜杠格式化仅在Windows生成路径分隔符；Unix合法反斜杠文件名保留。其它不支持的平台仍明确Disabled和执行拒绝。

正式脚本现在位于 tests/rust-native，进入当前候选源码摘要。原40脚本逐字保留（SHA13f28598ae86b56561e0690dadf3c6df5b99ebee9b8d3f565cb0cde874718cf5），独立六条原检查不变并增加失败非零退出，400层与取消四条保持原文。新Rust integration父 r06_native_discovery_current_source::macos_native_discovery_current_binary_real_run_and_boundaries 使用 env!(CARGO_BIN_EXE_lingxi-service)，不绑定任何/tmp旧bin。仓库外新建专属HOME/tmp/product，产品材料复制当前真实模板；读取当前源码内脚本并实际调用，检查子进程退出、全部50条收据、当前bin SHA、工具原始事件/持久历史/实际FD，并保存脚本SHA。三个原叶可共享此单一绑定key，避免每个叶重复执行整套。

Mac当前Cargo父50条PASS；原R04三工具套件11/11PASS；bin+正式父Clippy -D warnings、格式PASS。同步构建所需最新Main的Settings/Ticker/knowledge锁、18个生产依赖快照已记录，均不进入净patch；lib只含模块声明与工具注册两处配套，不覆盖其它作者改动。R2源码和1577...真实bin已另行冻结保留。

Windows编译检查是针对x86_64-pc-windows-gnu、rustc1.98.1、edition2021的生产模块+父调用点API/完整身份适配以及九项Windows测试metadata，-D warnings通过；不是完整service的Windows交叉构建、Windows链接或运行。API检查源也位于tests/rust-native，真实生产后端直接path导入，没有运行替身或假Run。Windows模块作者的strictClippy原始记录一并保留。

平台状态不能混写：macOS当前源码50+11实测PASS；Linux实现存在但运行NOT_RUN；Windows生产实现已接入、目标类型检查PASS，但Windows九项运行、真实Run/资源权限/取消闭环、链接与最终service完整交叉构建均NOT_RUN；远程UNC和Windows ARM64也未执行。当前Mac具名父在非Mac明确失败，不会以零条测试/静默跳过作为其它平台PASS。这些NOT_RUN不算三原叶全部跨平台完成，由主控后续平台验收如实处理。
