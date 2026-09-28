# R18 Windows 本地令牌消费端同根修复（静态交接）

## 范围与来源

原 R02 的本地凭证安全要求覆盖生产、桌面复用和 CLI 自动发现。此次具体漏洞在 R18 追加的桌面 Rust 入口 `rust-local-service.cjs` 和 CLI `rust-service.ts`：Windows 仅凭路径属性、`lstat` 或打开前后身份判定，令牌字节并未绑定到已核 owner、保护 DACL、普通文件和单硬链接的同一个 Windows 句柄。它是本轮后修引入，属于原 R02 合同范围；Windows 本机尚未复现。原始 R00 文件与 A16 原合同均未修改。

## 根因组与全部入口

| 入口或失败枝 | 本轮实现 | 静态结果 |
|---|---|---|
| 桌面新启动 READY 后读取 `instance.json`/`local-token.json` | `desktop/main.cjs` 把经安装包清单与签名核验或开发模式显式选择的固定 Rust 程序传给双记录读取 | 入口已接通；真实启动仍未执行 |
| 桌面已有 Rust 实例复用 | 同一个 reader 先读实例，再复读实例和令牌；随后仍须真实服务身份认证，安装包原有跨版本实例拒绝保留 | 入口已接通；真实复用仍未执行 |
| CLI 源码运行 | 固定优先使用 `dist-rust-service/win-<arch>` 且核清单/完整 SHA；阶段目录不存在时才使用源码树内固定 debug 程序；不采信 `LINGXI_SERVICE_BIN` 作 reader | 源码布局可静态定位；debug 程序无分发签名，仅适用于可信开发源码树 |
| 桌面安装包中的 CLI | 按 `resources/server/bundle/cli.js` 的实际位置查同级 `resources/rust-service`；核 PE 正文摘要与 `Lingxi.exe` 同签名者 | 损坏、异平台、错签名明确拒绝；真实签名包未在 Windows 执行 |
| 独立 LingxiCore CLI | `build-standalone-server-artifact.mjs` 把已核验的 Rust 阶段目录复制入 `LingxiCore/rust-service`；`verify-standalone-server-artifact.mjs` 解包后再核。CLI 按明确 `LingxiCore` 布局查完整 SHA；旁边偶有 `Lingxi.exe` 不改变类别 | 结构和篡改负例在非 Windows 主机通过；正式 Windows 归档未执行 |
| 宽 DACL、异主、重解析点、硬链接、文件/目录替换、读权限/IO 失败、读取中变化 | Rust helper 持有每级祖先、home、runtime 和最终文件句柄；按该句柄在读取前后核 owner、保护 DACL、类型、单硬链与长度；最终文件只分享读取、拒绝同时写/删；失败 exit 2 且不打印令牌，Node 不自动使用令牌 | 从源码看覆盖；Windows 实机失败分支仍未运行 |

Rust 子命令是 `lingxi-service --read-private-runtime-json <absolute-home> <instance|local-token>`，只在 Windows 可用。服务参数解析与启动路径保持既有行为。Node 只通过 `execFileSync` 参数数组调用，不用 shell，标准输出限 64 KiB；helper 的错误不带凭证。桌面原有真实 HTTP 身份检查是令牌读取之后的最终闭环。两次 helper 调用可能跨服务重启，实例 ID、PID、home、READY 地址与随后真实身份检查仍必须一致，失败拒绝连接。

## 当前验证

本机 Darwin arm64，锁定 Rust 1.98.1；`rustup target list --installed` 仅有 `aarch64-apple-darwin`。每个命令、UTC 开始/结束、退出码、原始 stdout/stderr、摘要与八个相关源码文件的前后 SHA-256 均在 [最终无端口批次](noport-final-2/summary.json)。批次期间这八个文件字节未变化。

| 检查 | 当前批次 |
|---|---|
| Rust 全 workspace fmt、check，以及 lingxi-service/lingxi-adapters 严格 clippy | PASS，三项 exit 0；均只编译本机目标 |
| `npm run typecheck` | PASS，exit 0 |
| CLI 与独立包定向 Vitest | PASS，2 文件 33 测试；包括打包布局、邻接同名 exe、二进制缺失/损坏/篡改拒绝 |
| 桌面 Rust 定向 Node 测试 | PASS，8 测试 |
| CLI esbuild ESM 打包、`git diff --check` | PASS，均 exit 0 |
| Windows 目标编译、真实 ACL/替换故障、签名安装包与独立包运行、真实 TCP 身份链 | BLOCKED／未执行：本机无 Windows 目标与环境，真服务动作仍受本任务平台边界；不得将本机 PASS 转写为 Windows PASS |

过程首次检查中，fmt 因排版 exit 1、TS 因 Windows 分支后的冗余条件 exit 2、独立包测试因新增目录期望未更新 exit 1，后续分别修正并复跑。另一次 `cargo --locked` 曾因并行源码和锁文件短暂不一致 exit 101；锁文件稳定后本最终批次 exit 0。这些首次输出仅存当次工具对话记录，没有独立原始日志文件，不能伪称已归档；最终批次原始日志单独保存，未覆盖上一批 `noport-final`。

## 尚未关闭的条件与风险

Windows 产品安全结论仍为 **BLOCKED**：需在真实 Windows x64 与适用 arm64 目标编译 helper，并逐一执行宽 DACL、异主、重解析点、硬链接、父目录替换、文件替换、读失败及读取中变化负例；验证桌面新启动/已有实例复用、CLI 源码与正式桌面安装包、独立 LingxiCore 的自动读取和真实身份响应。要保存各次原始日志和候选摘要。独立包的未签 Rust 程序依赖发布归档与安装目录的可信边界；若实际解包目录允许其他账户改写程序或清单，仍可能在启动 reader 前被替换，必须在 Windows 发布验收中证明目录不可由其他账户写入，或给独立包建立可核验的程序签名链。源码模式 debug 程序也以源码树可信为前提；缺失 reader 时明确拒绝自动令牌。

以上是 R02 内此问题组的静态修复与无端口结果，**不构成 R02 阶段 PASS 或进入 R03 的许可**。
