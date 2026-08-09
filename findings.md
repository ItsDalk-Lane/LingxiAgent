# 事故修复取证笔记

## 已知事实

- 任务书要求先取证、再修复，不能把“运行时未激活”误判成“Git 代码丢失”。
- 旧 `PROGRESS.md` / `progress.md` 明确记录仓库曾把版本改为 `0.1.3`，并将发布工作流切到正式版 Latest 路径。
- 旧 `BLOCKED.md` 记录上轮主工作树存在外部类型目录污染和若干非本任务改动；本轮必须重新核对当前状态，不能照抄旧结论。
- 任务书要求保留用户数据、有效 OTA、previous/rollback 和崩溃隔离语义；手工清缓存不算修复。

## 待验证关键点

- `0.1.3`、`0.1.21`、`0.1.22` 的真实 tag、提交、发布时间和所有构建元数据。
- 当前安装包 seed 与磁盘 current 的实际排序依据，以及 renderer/server 是否走完全相同的决策。
- 是否已有跨发布单调字段，可区分产品版本、构建、train 和 Artifact 世代。
- 所有正式 Release、Train、stable/beta pointer 的生成和写入顺序，失败后是否仍可能继续发布。
- 下游功能在 Git 中是否保留、是否有跨层协议断裂，以及运行时为什么没有体现。

## 证据来源

- `/Users/study_superior/.codex/attachments/88296525-a881-4242-accf-255bcb7d6950/pasted-text.txt`
- 仓库旧 `PROGRESS.md`、`progress.md`、`BLOCKED.md`

## 2026-08-09 初始现场

- 分支为 `main`，跟踪 `origin/main`；首次状态检查只看到本任务新建或追加的 `PROGRESS.md`、`findings.md`、`task_plan.md`，尚未发现其他当前工作树修改。
- 任务书共 1633 行；截至本次记录已读到第 1400 行，剩余交付清单仍需完整读取。
- 本地长期记忆索引中没有 LingxiAgent 或本次版本事故的相关记录；后续结论以当前仓库为准。
- 任务书明确要求发布工作默认止于 release-ready：不推 tag、不创建正式 Release、不发布 stable Train。
- 发布依赖链必须让版本预检、质量检查、Artifact 完整性、启动和历史升级 smoke 成为下游发布动作的真实前置条件。

## Git 与版本初证

- 当前分支 `main`，HEAD `b5d009d8b0ae1ab8b7df31edb0a102c98ad51b02`，与 `origin/main` 无领先或落后。
- 远程产品标签至少包括 `v0.1.0`、`v0.1.1`、`v0.1.2`、`v0.1.21`、`v0.1.22`、`v0.1.3`；`v0.1.3` 解引用到当前 HEAD。
- `train-stable-2`、`train-beta-5` 也指向当前 HEAD，说明错误版本不只进入安装包标签，也进入 Train 发布线。
- 根 `package.json` 与 lockfile 根包版本均为 `0.1.3`；`upstreamVersion` 为 `0.444.1`。
- 三个目标 pi 包的声明和 lockfile 解析条目均为 `0.84.1`，尚需验证实际安装目录与补丁脚本。
- upstream remote 由仓库事实确认是 `https://github.com/liliMozi/openhanako.git`。
- 仅按 Lingxi 产品标签比较，已进入远程的最高 SemVer 是 `0.1.22`，所以若无更高远程 Release，下一候选应为 `0.1.23`。

## 待补证

- GitHub Release 查询因 `gh release list` 不支持 `url` 字段而失败；应改用其报告的可用字段重试。
- `v0.1.3` 的 annotated tag object 是 `196d415d...`，发布提交是 `b5d009d8...`；时间线中要区分 tag object 与 commit。

## GitHub Release 实证

- `v0.1.3` 是非草稿、非 prerelease、当前 Latest，发布时间 `2026-08-08T16:10:07Z`。
- `v0.1.22` 是非草稿 prerelease，发布时间 `2026-08-06T15:04:16Z`；虽然是预览发布，但已有安装包和 Train，必须计入“可能进入用户设备”的版本上界。
- `v0.1.21` 同样是已发布 prerelease；`v0.1.2` 是此前正式版。
- `train-stable-2` 与 `train-beta-5` 都已作为 GitHub prerelease 发布，发布时间分别为 `2026-08-08T16:10:39Z` 与 `16:10:54Z`。
- 因此本次不能只修安装包标签，还要同时保护 stable/beta Train 路径。

## Artifact 源码入口初筛

- 启动与迁移：`desktop/src/shared/artifact-boot.cjs`、`desktop/src/shared/artifact-repair.cjs`、`desktop/bootstrap.cjs`。
- 核心状态：`shared/artifact-core/activation.cjs`、`manifest.cjs`、`pointer-store.cjs`、`pointer-channels.cjs`、`ota-core.cjs`。
- 在线更新：`desktop/src/shared/artifact-ota.cjs`、`train-update-apply.cjs`。
- 构建发布：`scripts/build-server-artifact.mjs`、`build-standalone-server-artifact.mjs`、`verify-seed-kit.mjs`、`publish-train.mjs` 与 `.github/workflows/*.yml`。
- 现有测试已覆盖 boot、activation、pointer、OTA、repair、seed 与 Train，但是否覆盖历史升级和发布阻断仍待逐个检查。

## 启动状态机根因证据

- `artifact-boot.cjs` 的当前规则是：没有可启动指针则激活 seed；发生崩溃回退时坚持 previous；否则直接比较 seed 与磁盘指针的产品版本，只有 seed 严格更大才激活。
- 对 `seed=0.1.3`、`current=0.1.22`，当前比较结果必为 seed 更旧，因此决策是继续启动磁盘 current。这与任务书事故假设一致，下一步用现有测试夹具跑出真实结果。
- 跨上游/下游版本号体系只用 minor 位数数量级判断并转入兼容规则；`0.1.3` 与 `0.1.22` 属于同一体系，不会触发该兼容分支。
- 旧指针没有可比较版本时：train 0 且内容哈希不同会自愈到 seed；train 大于 0 继续优先。该规则无法修复一个 metadata 完整但产品版本倒退的 `0.1.22` 指针。
- server 与 renderer 用独立指针命名空间，各自执行同构启动、失败计数和 previous 回退；这保留了独立回退能力，也允许两侧停在不同运行时。

## 当前元数据能力

- seed manifest 的 `train` 固定为 0；server 与 renderer 的 `version`、`minShell` 都直接来自根产品版本。
- Artifact 文件名分别是 `server-<version>-<platform>-<arch>.tar.gz` 与 `renderer-<version>.tar.gz`。
- manifest 已携带发布时间、内容哈希、大小、路径和契约版本，但初筛未见独立的安装包发布世代或来源提交字段。
- 在线 Train 复用同一 manifest 构造逻辑并分配各频道单调 train；train 号只在频道内有序，不能直接代表跨安装包 seed 的发布先后。

## 发布链缺口初证

- tag 触发的 `build.yml` 当前是 renderer box → 平台打包矩阵 → release → publish-train；`release` 只依赖 `build`。
- 平台打包会校验 seed kit，Windows 独立 server 还会做真实启动 smoke；但当前 tag 工作流没有把 typecheck、全量测试、lint、Artifact bootstrap smoke、历史升级 smoke 接成 release 的真实前置依赖。
- 普通 `ci.yml` 会执行 typecheck、lint、构建和全量测试，但它是 main/PR 的独立工作流，tag release 没有确定依赖它。
- 手工 `publish-train.yml` 直接调用 Train 脚本，目前未见候选产品版本相对历史 Release/频道指针的单调性预检。

## 事故复现（未改源码）

- 公开决策入口实测输入：current `{version: 0.1.22, train: 0, sha256: current-022, versionDir: /user/artifacts/renderer/0.1.22}`；seed `{version: 0.1.3, sha256: seed-013}`。
- 实测输出：决策为 `boot`，指针槽仍是 `current`，最终目录仍为 `/user/artifacts/renderer/0.1.22`。
- server 与 renderer 共用该纯决策规则，所以同样输入会让两者都留在 `0.1.22`；现有组合启动测试已证明两个 kind 都按此规则独立执行。
- 当前 `tests/artifact-boot.test.ts` 的 46 项全部通过，但其中“更旧安装包不得降级 current”把这一事故行为当成正常保护；缺少“错误产品版本后由更高发布世代修复”的表达能力。

## 本地验证环境

- 默认 `node` 是 `/Users/study_superior/.local/bin/node`，版本 `v22.23.2`，不满足仓库 Node 24 约束。
- 本机已有 `/Users/study_superior/.nvm/versions/node/v24.15.0` 与 `v24.16.0`；后续正式命令使用仓库 CI 同款 `v24.15.0` 的绝对路径优先。

## 状态机设计约束（源码细读）

- server/renderer 启动结果当前只返回目录、train、产品版本、槽位、是否激活 seed、是否崩溃回退、隔离 train 和回退前后版本；没有把内容哈希、发布时间或发布来历带到主进程。
- 每侧启动都会先晋升 next，再检查连续失败；达到阈值后先降到 previous，并明确跳过 seed 新鲜度覆盖。这一安全语义必须原样保留。
- seed 激活走与 OTA 相同的归档验签、解包和指针写入路径；修复排序不需要另造激活通道。
- OTA manifest 要求同一 train 的 server/renderer 产品版本一致，并已有内容哈希一致、同版不重复应用、产品版本不倒退和契约兼容检查。
- 当前构建 manifest 的统一来源只接收产品版本、平台、架构、签名 key、发布时间和两种归档信息，确认没有传入来源提交或独立发布世代。
- 主进程实际从已解析的版本目录启动 server，并把已激活 renderer 目录传给 server；因此错误指针会同时影响桌面页面和 server 供给的远程页面，绝不只是 About 显示问题。

## 历史发布与线上频道状态

- 产品链：`0.1.2`（pi 0.80.3）→ `0.1.21`（pi 0.80.3）→ `0.1.22`（pi 0.83.0）→ `0.1.3`（pi 0.84.1）→ 当前 main（同 `0.1.3` 提交）。
- 四个产品 Release 都真实携带 renderer 归档和 darwin arm64/x64、linux x64、win32 x64 server 归档，文件名产品版本与 tag 一致。
- 当前线上 `stable.json` 是 train 2、产品 `0.1.3`；`beta.json` 是 train 5、产品 `0.1.3`。旧 beta train 4 是产品 `0.1.22`。
- 线上 signed channel/Train manifest 只有 train、频道、发布时间、契约、产品版本和归档信息；没有来源 Release 提交或跨 seed/Train 的发布世代。
- 现有 Train 发布只防 train 号回退、同版不同内容和错误频道；当频道是 `0.1.22` 时，候选 `0.1.3` 属于“不同版本”，脚本没有比较 SemVer，因而允许 stable/beta pointer 降到 `0.1.3`。
- 客户端 OTA 又有产品版本不倒退保护，所以一个已在 `0.1.22` 的客户端既不会接受线上 `0.1.3` Train，也不会接受 `0.1.3` 安装包 seed，形成双通道锁死。

## 发布世代设计结论

- 现有 train 只在单一频道内单调，`releasedAt` 是 Train/构建发生时间，无法稳定表达“这个 OTA 来自哪个产品 Release”；二者都不能直接承担跨安装包与 OTA 的统一顺序。
- 需要一个最小的、由源码发布元数据维护的单调 `releaseGeneration`，同时写入 seed、Train、激活 receipt/指针和日志；`sourceCommit` 只做来历识别，不参与大小排序。
- 为兼容旧客户端与旧指针，新字段必须在 schema 1 中可选读取；当前及后续正式构建必须由预检强制存在、递增并与候选 tag 一起发布。

## 上游三方审计坐标

- 仓库 remote 明确：`upstream=https://github.com/liliMozi/openhanako.git`。
- 上游 `v0.443.54` 为 `4cfdb13f...`，目标 `v0.444.1` 为 `cc19cb49...`；两标签间共 52 个路径变化。
- Lingxi 同步前状态 A：`a5d1e541...`，已经包含 `0.1.22` 之后的模型设置、Ollama、审批、语义槽位等下游提交。
- 上游目标 B：`cc19cb49...`；由于 Lingxi 产品历史与上游线没有共同祖先，无法普通 merge。
- 同步结果 C：`97595264...`，单父提交；提交说明记录采用 BASE/OURS/THEIRS 逐文件三方合并并补齐 pi 0.84.1 适配，随后 `b5d009d8...` 只准备 `0.1.3` 正式发布。
- `97595264` 修改 71 个文件；需要对上游 52 个路径和 A 的下游高风险区域逐项交叉验证，不能只引用提交说明。

## SemVer 工具

- 依赖树里已有多个 `semver` 7.x，但根包未直接声明；发布预检若使用它，应增加直接开发依赖，避免依赖传递依赖的提升布局。

## 下游功能提交清单

- `cce8e863`：修复纯 skill 消息被门禁丢弃，9 文件。
- `d555c14e`：供应商、模型、用量设置界面重构，19 文件。
- `34dbb17d`：Ollama 上下文、工具声明、结构化输出、thinking 与上下文长度桥接，15 文件。
- `0250f5fc`：审批收口为单一意图审查者，7 文件。
- `283d9581`：title、summarize、memory、vision、approval、guard 六类语义槽位体系，64 文件。
- `a5d1e541`：语义槽位跨层收口修复，25 文件。
- `97595264`：上游 0.444.1 与 pi 0.84.1 同步，71 文件。
- `b5d009d8`：`0.1.3` 发布准备和审计修复，28 文件。
- 当前树能找到上述各领域的生产入口和专门测试，但还需运行定向测试并检查跨层读写，不以文件存在直接判定功能完整。

## 审计脚本问题

- 首次 A/B/C 路径分类脚本错误地把连 `package.json` 在内的所有路径都标为 missing，明显违反已知 Git 事实；该结果作废，不能用于审计结论。

## A/B/C 重跑与功能基线

- 改用显式 bash 后阳性校验通过：A、B、C 的 `package.json` 都可读取；上游 52 个变更路径在 C 中全部存在。
- 上游新增的本地轻量压缩、内部情绪块及其测试在 C 中与 B 逐字节一致；多处事件、协议、桥接、样式和实验模块也与 B 一致。
- 其余 B≠C 路径主要是 Lingxi 品牌、下游设置/语义槽位、现有测试和生成清单的合并点，需以定向测试与源码链路判断，不能要求整文件等于上游。
- Node 24.15.0 下执行 36 个功能测试文件：36 passed，661 tests passed，0 failed。
- 该基线同时覆盖语义槽位、独立 approval、Gateway、模型同步/提供方、Ollama、skills、配置归属与迁移、上游轻量压缩/情绪块/桥接、pi 边界、xAI OAuth 适配和设置 UI。
- 偏好迁移测试中的 stderr 是用例主动注入损坏 JSON 后验证“保留原始字节再替换”的预期日志，测试通过，不是失败。

## 已发布产物抽检状态

- 本地只有陈旧的 `renderer-0.1.21.tar.gz`，不能用于证明 `0.1.3` packaged state。
- 首次从 GitHub 下载 `0.1.3` renderer/server 资产时临时下载链接返回 EOF，文件未落盘；后续哈希和内容检查没有输入，因此无效且未形成结论。

## `0.1.3` packaged state 实证

- renderer 归档重试下载成功，SHA-256 为 `5806dad27c9f84eb08ed8a4c80845f923d5e38c36fdfacbb60e54e757bd3f083`，与 signed stable/beta Train 完全一致。
- renderer 包内能找到新版 `/api/providers/summary` 设置数据源、`auxiliary-vision` 调用和 skillBadge 处理代码。
- darwin-arm64 server 归档下载成功，SHA-256 为 `8891b8872fbf8274de0919bcf50117fdf7d1c9dfe6721d2bb80c09129a42a8db`，与 signed stable/beta Train 完全一致；包内 package version 为 `0.1.3`。
- server bundle 内能找到 approval/title/summarize/memory/vision 模型键、供应商汇总路由、Ollama `num_ctx`、skill 消息和上游轻量压缩标记。
- 结论：Source state 与 Packaged state 都包含用户报告“消失”的新功能；Active runtime state 因指针选择 `0.1.22` 才表现为旧界面/旧服务端。

## 修复实现与迁移验证

- 根版本已改为 `0.1.23`，根 lockfile 同步；直接生产依赖固定 `semver@7.8.0`。
- `releaseGeneration=1` 与来源提交进入签名 seed/Train、指针和 `.verified` 回执；旧 schema/指针缺字段仍可读取。
- 启动选择保持崩溃回退最高优先级、同版本/同内容幂等；新世代首次遇旧指针会接管，进入世代规则后旧元数据不能倒灌。
- OTA 同步使用发布世代，缓存状态保存世代与来源提交；server/renderer next 还必须在 train 和世代上同时一致。
- 启动日志结构化报告壳、server、renderer 的版本、世代、train、哈希、槽位、来源与提交；混合世代或新旧元数据混合会告警，但不破坏独立回退。
- `npm run test:artifact-release-smoke` 首轮：8 文件、303 项全绿；新增 prerelease 和工作流守卫后需最终重跑记录新数字。
- 真实集成用例从 legacy `0.1.22` 与 legacy `0.1.3` 分别升级，两侧都激活 `0.1.23` generation 1；干净安装、同版重装、旧 Train、crash fallback 均覆盖。

## 发布门禁

- `release-preflight` 枚举全部已获取标签，只把包名、tag 与该标签 `package.json` 版本三者一致的 Lingxi 标签算作产品历史，避免把断开的上游 0.4xx 标签混入；真实仓库结果为历史最高 `0.1.22`、候选 `0.1.23` PASS。
- Build workflow 的 Release 现在依赖：版本预检 → 平台构建与全量质量 → Artifact/历史升级 smoke → Release；缺一不会进入发布 job。
- 自动和手工 Train 都在签名密钥落盘/远程写入前重跑预检；发布脚本再比较现有签名频道的发布世代。
- 预发布 tag 通过 `semver.prerelease` 分类为 prerelease 且不是 Latest。
- 工作流结构守卫 5 项、既有 CI/seed 守卫 18 项均通过。

## 环境与剩余发布阻塞

- `npm ci` 后 Node 24 三段类型检查 0 error；lint 0 error、7958 个历史 warning，退出码 0。
- 三个目标 pi 包实际安装目录和 `npm ls --depth=0` 均为 `0.84.1`；`scripts/patch-pi-sdk.cjs` 输出 all checks passed。
- release digest 仍是 `0.1.3`。生成器需要未配置的 `OPENAI_API_KEY`，且必须在本次改动形成提交后采集提交范围；遵守项目规则没有手改，已在 `BLOCKED.md` 写明解除步骤。

## 最终验证

- Node 24.15.0 全量测试最终为 1088 个文件通过、1 个跳过；11016 项通过、7 项跳过、0 项失败。
- Artifact release smoke 最终为 8 个文件、304 项全绿；工作流/既有 CI/seed 守卫为 3 个文件、23 项全绿。
- `build:packages` 与 `build:client` 退出码均为 0；darwin-arm64 `build:server` 使用一次性本地密钥生成签名 seed，`verify-seed-kit` 通过，临时私钥目录随后已移除。
- 解包后的 darwin-arm64 server 在隔离用户目录真实启动，身份接口返回 HTTP 200、协议 1，包内版本为 `0.1.23`。
- 最终三段 typecheck 为 0 error；lint 为 0 error、7958 个仓库历史 warning；两份旧摘要对 `v0.1.23` 的校验都按预期以退出码 1 阻断。
- `git diff --check`、受控 JSON 解析和待交付文件秘密标记扫描通过；最终 Git 清单只含本任务源码、测试、生成治理清单、计划和报告。

## 正式发布续跑前置

- 用户已明确授权修复剩余阻塞、提交、创建 tag、推送并创建正式 Release，不再停留在“只准备仓库”边界。
- `gh 2.97.0` 可用，已登录 `ItsDalk-Lane`，权限包含 `repo` 与 `workflow`；`origin` 指向 `ItsDalk-Lane/LingxiAgent`。
- 当前仍在 `main`，与 `origin/main` 0 ahead / 0 behind；39 个工作区变化均来自本事故修复链，尚未提交。
- 当前 shell 未配置 `OPENAI_API_KEY`、`GH_TOKEN` 或 `LINGXI_SIGN_KEY`；GitHub CLI 可用 keyring 凭据，摘要生成器的模型凭据仍需继续核对。
- 文件化计划的 session catchup 命中一段与本仓库无关的旧 Claude 技能问答，没有需要合并进当前计划的代码事实。

## 发布前依赖安全复核

- 实时 `npm audit --omit=dev` 为 33 项：0 critical、15 high、18 moderate；完整依赖树为 47 项，其中 2 critical 来自开发/构建依赖，不进入生产依赖统计。
- 生产 high 不是纯历史噪声：直接涉及 Lark SDK、自动更新、Hono、js-yaml、undici，并均有当前兼容版本修复；完整树 critical 涉及 Vitest 与 tar，当前依赖链也有修复版本。
- 多数修复位于已声明的 caret 范围内，只因 lockfile 仍锁旧版；少数明确固定版本需要窄幅升级：Electron 42.3.0→42.8.1、Mermaid 11.10.1→11.16.1、undici 7.24.7→7.29.0。
- exceljs 与 node-telegram-bot-api 的剩余 moderate 只给出不安全的降级/主版本升级方案，不在未验证情况下强行修改；目标是先消除所有 critical/high，再按完整回归判断发布。
- 直接依赖和 lockfile 安全更新后，生产树降为 9 项 moderate、0 critical/high/low；完整树为 9 项 moderate、1 项 low、0 critical/high。原有 2 critical 与 24 high 已全部消除。
- 剩余项集中在 Hono WebSocket 适配、Excel/Telegram 的旧请求链和构建器；审计工具只给出破坏性升级、降级或尚无修复版本，因此没有使用强制修复冒险改变产品行为。
- 实际 `npm ci` 暴露出 `yauzl` 原本没有被项目直接声明，只是旧打包工具把它提升到了根目录；打包工具升级后生产代码和类型检查一起报缺包。这不是测试框架不兼容，而是隐藏依赖被可靠地揭露，现已把运行依赖和类型依赖显式归属到项目。
- 修复隐藏依赖后，全量测试重新达到 1088 文件通过、1 文件既有跳过，11016 项通过、7 项既有跳过、0 失败；类型检查与 lint 也恢复通过，说明安全升级没有改变既有业务基线。
- 升级后的独立服务端真实装包并运行：签名 seed 自验成功，隔离启动后的身份接口返回 HTTP 200、协议 1、版本 `0.1.23`。这覆盖了 Lark、网络、打包和原生依赖变动实际进入发布包的路径。

## DeepSeek 摘要生成迁移

- 用户明确选择 `DEEPSEEK_API_KEY`、`deepseek-v4-flash` 和 DeepSeek Responses API；不能只替换现有 `--model`，因为当前实现还把 OpenAI 域名、密钥名与响应提取格式写死。
- 安全约束：密钥只进 Authorization 请求头，不写来源包、摘要、日志或错误；模型返回值必须继续通过本地严格 schema、tag/version 和 v2 历史顺序校验。
- 2026-08-09 官方资料确认 `deepseek-v4-flash` 存在、OpenAI 格式 base URL 为 `https://api.deepseek.com`，且支持 JSON Output；公开 V4 公告和接口参考明确列出的却是 OpenAI Chat Completions 与 Anthropic API。
- 对 DeepSeek 官方文档做 `Responses API`、`POST /responses`、`previous_response_id` 和 `responses.create` 精确检索均无结果。该能力与用户说明存在证据差异，不能未经端点实测直接落实现有 OpenAI Responses 请求体。
- 对 `/responses`、`/v1/responses`、Chat Completions 和故意伪造路径做无密钥/假密钥探测，DeepSeek 网关都在路由判断前统一返回 401，OPTIONS 也统一返回 200；这些响应无法证明 Responses 路由存在。
- 当前进程没有 `DEEPSEEK_API_KEY`，所以暂时不能用真实鉴权请求裁决 Responses 请求体。实现前还需查官方文档目录；若官方契约仍缺失，应显式阻断或改用已文档化的 Chat Completions，而不能把猜测当正式发布依赖。
- DeepSeek 官方 `sitemap.xml` 实际列出了 `/guides/responses_api` 与 `/api/create-response`；搜索索引滞后造成了前述漏检。用户关于 Responses API 的说明成立，后续以这两份官方页面的请求/响应契约实现，不改用 Chat Completions。
- 官方 Responses 契约：`POST https://api.deepseek.com/responses`，当前只支持 `deepseek-v4-flash`；`instructions`、`input`、`max_output_tokens` 和 `text.format` 均受支持。
- `text.format` 支持 `json_schema`、`name` 与 `schema`，但官方字段没有 `strict`；现有 OpenAI 请求中的 `strict: true` 不应继续发送。服务端结构化输出后仍须执行仓库本地 `assertValidReleaseDigest`。
- 非流式响应是 Responses 结构：`status` 可为 completed/incomplete/failed，正文位于 message 项的 `output_text` content；生成器应拒绝非 completed 状态，不能从失败/截断响应里捞部分正文。
- DeepSeek Responses 是无状态接口，`store` 不支持且永远为 false；摘要是单轮生成，不需要 `previous_response_id` 或 conversation，现有请求也无需这些字段。
- 实现只改摘要生成边界：密钥改为 `DEEPSEEK_API_KEY`，默认模型改为 `deepseek-v4-flash`，端点改为官方 `/responses`；原有来源采集、v1 schema、本地校验和 v2 史册追加逻辑保持不变。
- 远端非 2xx 错误不再附带响应正文，避免服务端异常回显进入日志；非 completed 响应即使带有看似完整的部分 JSON 也会失败关闭。
- 无密钥的真实命令验证为失败关闭：退出码 1、错误明确指向 `DEEPSEEK_API_KEY`、目标摘要文件不存在。
- 真实 Responses 输出可先给出 reasoning 项，再给 message/output_text；不能遍历到任意 `text` 就返回，否则会把思考过程送进 JSON 解析。最终提取只接受 `output_text`。
- 修复后真实生成成功：v1 为 `0.1.23`，v2 头部为 `0.1.23` 且保留三条旧记录；两个校验器都退出 0，模型条目的三个 commit ref 均能在本地 Git 解析。
- 摘要进入工作树后，版本预检、304 项 Artifact release smoke 与 11021 项全量测试全部通过；因此摘要变化没有破坏发布链或包内历史读取。
- 首次 tag CI 暴露本机无法发现的测试夹具问题：Ubuntu 上 Computer Use 正确判为不支持，但 6 项路由正向用例和 2 项引擎懒加载用例没有固定受支持平台。生产行为无需修改，测试必须显式声明 darwin；现有 Linux 负向用例继续证明不支持路径。
- `v0.1.23` 的四个平台构建成功，但 quality 失败使 Release、Artifact smoke、Train 全部 skipped；远端未产生 Release。由于 tag 已推送且禁止重写，下一候选必须是 `0.1.24` / generation 2，不能强行复用 `0.1.23` / generation 1。
- 测试夹具与版本/世代修正后，本地 preflight、304 项 Artifact smoke、11021 项全量测试、类型和 lint 均通过；生产代码的平台支持判断未被放宽。
- DeepSeek 在带完整发布事实的摘要任务上以 `incomplete/max_output_tokens` 结束，4000 额度不足；失败响应没有覆盖先前摘要。提高额度比接受部分正文或删除事实约束更安全。
