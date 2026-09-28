# R18 Windows 令牌消费者：真实分发路径补修

## 范围与来源

- R18-N01 属 R02 原安全要求；桌面和 CLI 消费者是本轮后修新增，真实激活目录漏项、版本混用及源码旧 stage 则是该后修的同根因遗漏。R00 原件未改。
- 本次把 Windows reader 纳入 full server 签名归档、OTA 激活/回退版、Windows 独立包和 CI 发布顺序；源码 CLI 只接受当前源码对应的已构建 stage。`build:server:open` 属 `PROGRESS.md` 封印构建与 A16 旧入口回归，但 R02 原任务没有要求它提供 Rust 自动令牌功能；此入口目前在 Windows 对 Rust 自动读令牌拒绝，完整 open 构建仍未在当前候选运行。
- Windows 真机、Windows Rust target 和已安装包均未在本机验证。此处没有真 TCP、完整 npm、A16 或完整 verify-stage 结果。

## 已改入口与失败枝

| 入口 | 本轮静态修正 | 明确拒绝的失败枝 |
|---|---|---|
| 发布 CI、`dist:win`、`build:server`、`packServerArchive` | 当前 Rust stage 在 server 装箱前构建并按源码、工具链、版本核验；reader 与 Node CLI 同树进入签名 server 归档 | 无 stage、旧源码、错版本、缺 reader、被改写的 reader |
| OTA 当前版、回退版 CLI | 从实际 `artifacts/server/<version>-win32-<arch>/rust-service` 查找；核 versionDir、同包 `package.json`、`.verified` 与 current/previous 指针，再核 reader 清单和完整程序摘要 | 错布局、无指针、错版本、receipt 不符、缺失/改写 reader |
| Windows 独立包构建、解包复验、运行 CLI | reader 位于 `LingxiCore/server/rust-service`；外层版本、Node 服务版本和 Rust 清单版本在三处核对 | 缺程序、错版本、外层/服务/reader 版本混用、程序字节变化 |
| 源码 CLI | 固定 `dist-rust-service/win-<arch>`；核当前 Rust 源摘要、工具链和产品版本；不回退到 debug 程序 | 旧 stage、缺 stage、错误架构、清单或二进制损坏 |

新代码注释为中文；macOS/Linux 令牌读取路径未改。CLI 自动读取失败返回明确错误，不自动使用未经核验的本地令牌。

## 当前候选的无端口定向检查

原始记录在 [`noport-final/summary.json`](noport-final/summary.json)，每项包含真实命令、UTC 起止、退出码和 stdout/stderr 摘要；同目录保存各项原始 stdout/stderr。候选十个相关源码文件的前后 SHA-256 完全一致。macOS 主机上共 9 项退出码均为 0：`npm run typecheck`；3 个 Vitest 文件 74/74；桌面 Node 测试 8/8；四个脚本语法检查；CLI bundle；`git diff --check`。首次 72 项通过的原始日志仍留在本目录根部，没有覆盖成后来 74 项结果。

这些是本机无端口检查，不证明 Windows ACL、Windows 可执行文件、正式签名包或实际服务连接通过。

## 未关闭的产品与验收缺口

1. **已激活目录的发布信任仍 FAIL。** `shared/artifact-core/activation.cjs` 在激活时核归档 SHA，但启动时只信 `.verified` 与指针，不重核已解压文件树。若其他 Windows 账户可修改已激活目录，它可同时替换 CLI、reader、相邻 `build.json`、receipt/指针；当前消费者的相邻摘要和 receipt 不构成独立信任根。需要在激活/升级/回退实际入口证明并强制版本目录与所有祖先仅归当前用户或受信账户写入，或在执行前由不可替换的签名安装组件重验包签名和实际程序句柄。还须在 Windows 实机注入宽权限、重解析、替换和并发替换负例。
2. **Windows 独立包外部信任仍 FAIL。** 外层 manifest 和 archive 当前都没有独立签名。两者可一起改写；自洽 SHA 只能发现单边损坏。正式发布需要已有钉住的签名体系覆盖该产物，或由签名安装器/启动器提供等价不可替换信任根，再实测篡改拒绝。没有发布密钥和 Windows 正式包，不能把静态摘要检查写成信任链 PASS。
3. **Windows 平台验证 BLOCKED。** 本机只有 macOS Rust target；Windows 构建、旧/新服务目录 ACL、桌面新启动/实例复用、源码/激活/回退/独立包 CLI 和真实令牌读取都未运行。需要 Windows runner/实机、当前候选的正式产物与原始日志。完整 R02 Gate、A16、34 原叶和封印由总控另行判定。

因此 R18-N01 的分发布局和版本混用漏项从代码看已修、无端口定向检查通过；跨账户后续篡改与独立包发布信任尚未关闭，Windows 运行未验，不能据此将 R02 标为 PASS。
