# PROGRESS — 对抗性审计、修复与 0.1.3 正式发布

- 审计范围：`cce8e86..97595264`，6 个提交、153 个文件；任务 1–6 全部完成。
- 修复前发现：P0 0 项、P1 1 项、P2 2 项、P3 3 项；详见 `AUDIT-REPORT.md`。
- 已修复审批材料凭证泄露，并验证模型不能绕过本机终裁。
- 已修复 Ollama 能力来源、无界并发和不安全上下文参数。
- 已修复依赖副作用导入守卫、旧品牌测试模拟和运行时品牌默认值。
- 生成产物已用仓库工具同步；持久化版本仍为 3，数据纪元仍为 1。
- Node 24 全量测试：1085 个文件通过、1 个按平台跳过；10986 passed、7 skipped、0 failed。
- 相关 197 个定向测试、发布摘要 v1/v2 校验、边界检查与 `git diff --check` 均通过。
- 主树类型检查受 `node_modules/@types/* 2` 外部重名目录阻断；相同代码已在干净工作树验证 0 error。
- 版本已改为 0.1.3；发布工作流已切换为正式版、非预览、Latest 路径。

---

# PROGRESS — `0.1.3` 发布事故修复与防复发

## 2026-08-09：任务启动

- 已读取仓库规则、任务书前半部分以及旧进度和阻塞记录。
- 已按复杂任务要求建立 `task_plan.md` 与 `findings.md`，当前处于“建立事实与事故复现”阶段。
- 当前尚未修改业务代码；`0.1.23` 仅为任务书给出的候选修复版本，须先枚举真实发布版本后裁决。
- 初始 Git 事实：`main` / `b5d009d8b0ae1ab8b7df31edb0a102c98ad51b02`，与 `origin/main` 同步；远程已有 `v0.1.21`、`v0.1.22`、`v0.1.3`，且 stable/beta Train 已指向 `0.1.3` 发布提交。
- 根包和 lockfile 当前产品版本均为 `0.1.3`，`upstreamVersion=0.444.1`；三个目标 pi SDK 声明与 lockfile 条目均为 `0.84.1`。
- 命令 `gh release list --repo ItsDalk-Lane/LingxiAgent --limit 100 --json ...url` 退出失败：当前 gh 不支持 `url` 字段；已记录并准备使用支持字段重试。
- 重试成功：`v0.1.3` 是当前 Latest 正式 Release；`v0.1.22`、`v0.1.21` 虽标为 prerelease，但都已真实发布并可能写入用户 Artifact 指针。stable/beta Train 也已发布到 `0.1.3` 提交。
- 已定位 Artifact 启动、指针、激活、OTA、构建、seed 校验与 Train 发布的主要源码和测试入口；下一步收窄搜索范围，建立真实状态机与事故复现。
- 源码确认当前启动决策直接比较 seed 与磁盘指针的产品版本，只有 seed 严格更新才激活；因此 `0.1.3` seed 会输给完整的 `0.1.22` current。旧指针兼容和跨体系启发式都不会覆盖本事故。
- 源码确认 seed 的 renderer/server 与 Artifact 文件名都沿用根产品版本，Train 只有频道内单调序号；当前 manifest 未发现独立发布世代。
- tag 发布链目前只要求平台 build 成功；普通 CI 的类型检查、测试和 lint 与 tag release 没有确定依赖，且未发现历史用户升级 smoke 作为发布前置门。
- 未改源码复现成功：`current=0.1.22/train0`、`seed=0.1.3` 时公开决策入口返回 `boot`，最终目录仍是 `0.1.22` current；现有 Artifact boot 46 项测试全部通过，说明测试把产品版本当成唯一跨安装包顺序，无法表达本事故的修复发布。
- 默认终端 Node 为 `v22.23.2`，不满足项目约束；已确认本机存在 CI 同款 Node `v24.15.0`，后续正式验证切换到该绝对路径。
- 状态机细读确认：seed 与 OTA 共用激活链，崩溃回退会刻意禁止 seed 立即顶回；当前启动结果缺少哈希和发布来历，manifest 也没有独立安装包发布世代。修复应扩展现有 manifest/指针，而不是另建激活路径。
- 线上取证确认 stable train 2 和 beta train 5 都已降到产品 `0.1.3`；beta train 4 是 `0.1.22`。Train 发布端没有 SemVer 回退检查，而客户端 OTA 与 seed 启动端都会拒绝 `0.1.3 < 0.1.22`，受影响用户被两个更新通道同时锁在旧运行时。
- 已裁决使用最小 `releaseGeneration` 作为跨 seed/OTA 的发布顺序，并携带 `sourceCommit` 仅作来历；旧 schema/指针保持兼容读取，正式发布预检负责强制新字段单调递增。

## 版本时间线（本地 Git + GitHub Release 实测）

| 节点 | 发布提交 / parent | GitHub 发布时间 | package | renderer / server Artifact | Train | upstreamVersion | pi SDK |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `v0.1.2` | `83037b06` / `46d77e37` | `2026-08-05T17:05:05Z` | `0.1.2` | `renderer-0.1.2` / `server-0.1.2-*` | beta 2 | 元数据尚未建立 | 三包 `0.80.3` |
| `v0.1.21` | `8840d1ab` / `362f815c` | `2026-08-06T05:42:54Z` | `0.1.21` | `renderer-0.1.21` / `server-0.1.21-*` | beta 3 | 元数据尚未建立 | 三包 `0.80.3` |
| `v0.1.22` | `a071fce7` / `e9c76127` | `2026-08-06T15:04:16Z` | `0.1.22` | `renderer-0.1.22` / `server-0.1.22-*` | beta 4 | `0.443.46` | 三包 `0.83.0` |
| `v0.1.3` | `b5d009d8` / `97595264` | `2026-08-08T16:10:07Z` | `0.1.3` | `renderer-0.1.3` / `server-0.1.3-*` | stable 2、beta 5 | `0.444.1` | 三包 `0.84.1` |
| main | `b5d009d8` / `97595264` | 未新增发布 | `0.1.3` | 构建输入仍为 `0.1.3` | 指向上述事故 Train | `0.444.1` | 三包 `0.84.1` |

标准 SemVer 从左到右比较数字段，所以 `0.1.22` 的 patch 为 22，严格大于 `0.1.3` 的 patch 3；发布先后时间不会改变该优先级。

## 版本序传播图

```text
package.json 产品版本
  → renderer box / server box 文件名与条目版本
  → 每个平台签名 seed manifest（train 0）
  → Electron 安装包 Resources/seed
  → 首启验签与 seed/current 比较
  → next → current → previous 指针
  → 实际 renderer 加载目录 + server 启动目录

同一批 Release 归档
  → Train 发布脚本重新组装签名 manifest
  → stable/beta channel pointer
  → 客户端 OTA 比较 train、产品版本、契约与哈希
  → next 指针
  → 下次启动晋升为 current
```

事故点有两个：发布端允许 `0.1.3` 覆盖 `0.1.22` 频道；客户端 seed 与 OTA 端又正确地把 `0.1.3` 当成降级拒绝，导致安装包已经换新但真实运行目录仍旧。

## 上游同步审计基线

- A（同步前 Lingxi）：`a5d1e5415c28b55074ba9ae81a6429d57ff5a934`
- B（上游目标）：`cc19cb49b0786d61ed723764e0a83baf87887270`（`v0.444.1`）
- C（同步结果）：`97595264ead8735a04559507ddaade25db8a4e15`
- 当前交付基线：`b5d009d8b0ae1ab8b7df31edb0a102c98ad51b02`
- A/B 无共同 Git 祖先，原同步按文件三方合并；本轮重新以文件差异、当前源码和测试验证，不把 `upstreamVersion` 或旧进度当作完成证明。
- 已枚举 `0.1.22` 后 8 个实际产品提交及其范围；模型设置、Ollama、审批、六类语义槽位、skills 消息、上游同步与发布准备均有可追踪生产文件和测试入口。
- 首次 A/B/C 自动分类因 shell 解析错误把所有路径误标为不存在，结果已废弃并记录；尚未据此作任何功能结论。
- A/B/C 重跑成功：上游 `0.443.54→0.444.1` 的 52 个变更路径在同步结果 C 中全部存在；新增轻量压缩、情绪块等关键模块与上游逐字节一致，其余冲突区进入下游行为验证。
- Node 24.15.0 定向功能基线：36 个测试文件、661 项断言全部通过，覆盖模型设置、辅助模型六类槽位、approval/Gateway、Provider/Ollama、skills、配置迁移、上游同步功能和 pi 0.84.1 适配。
- `0.1.3` 已发布归档首次下载因 GitHub 临时链接 EOF 失败；未把失败后的空目录当成 packaged-state 证据，已记录并改用 fail-fast 单资产重试。
- 已发布 `0.1.3` 产物抽检成功：renderer/server 哈希分别与 signed Train 一致；包内实际包含新版供应商设置、辅助模型、approval、Ollama、skills 和上游轻量压缩代码。已明确区分 Source、Packaged、Active runtime：代码和包没有丢，客户端激活状态错了。
- 阶段 1、2 完成：已生成 `UPSTREAM_SYNC_AUDIT.md` 与 `FUNCTIONAL_REGRESSION_AUDIT.md`。结论是 0.444.1 实际行为已进入当前树，Lingxi 下游功能在源码和发布包中都存在；当前转入版本与 Artifact 排序修复。

## 2026-08-09：版本、Artifact 与发布门禁修复

- 修复版本确定为 `0.1.23`；根包与 lockfile 一致，产品历史最高版本实测为 `0.1.22`。
- 新增签名发布世代 `1` 和来源提交；seed、Train、指针、验收回执、启动日志与 OTA 使用同一来历数据。
- 迁移性质：新世代接管旧指针；带世代指针拒绝旧元数据；同版/同内容幂等；三连败 previous 回退永远优先。
- 混合运行时保留两侧独立回退，同时对世代不一致、新旧元数据混合和旧版本混合写明确日志告警。
- `npm run release:preflight` 实测 PASS：候选 `0.1.23`、历史最高 `0.1.22`、候选世代 1、历史世代 0、识别 6 个历史产品标签。
- 负向自动测试真实复现并拒绝 `0.1.3 <= 0.1.22`；tag/包/lock 不一致和世代复用也会失败。
- Build Release 依赖链已改为：preflight → quality/platform build → Artifact release smoke → Release；自动/手工 Train 在远程写前再次预检并检查签名频道世代。
- Artifact release smoke 首轮：8 文件、303 tests passed；工作流结构、CI 既有守卫和 seed 校验补充测试：3 文件、23 tests passed。
- 初次类型检查被 `node_modules/@types/* 2` 污染；按 lockfile `npm ci` 后原命令通过，三段 TypeScript 均 0 error。
- `npm run lint` 退出 0：0 errors、7958 个仓库历史 warnings；没有借本任务清理范围外警告。
- 三个 pi SDK 实际安装版本与 `npm ls --depth=0` 均为 `0.84.1`；补丁校验脚本输出 `[verify-pi-sdk] all checks passed`。
- 已新增 `INCIDENT_REPORT.md`、`RELEASE_VERSIONING.md`；`BLOCKED.md` 记录 release digest 仍需在提交后用仓库生成器生成，当前环境无 `OPENAI_API_KEY`，因此发布校验应继续阻断 tag。

## 2026-08-09：最终验证与交付状态

- 受控运行闭包已用 `scripts/compute-cli-closure.mjs` 重生成：10605 个文件，其中源码图 664、运行资产 11、运行时追踪 9930；开放边界只保留 1 条既有待证据边，新增发布排序文件已进入开放清单。
- `desktop/main.cjs` 只增加只读运行时来历日志，没有改变持久化字段或数据结构；已按“兼容变更”重封持久化指纹，数据纪元仍为 1，相关 15 项 tripwire 全绿。
- 首轮全量发现 4 个文件、14 项失败：6 项是旧启动契约夹具缺新日志依赖，8 项是新增运行闭包/边界/指纹尚未同步；逐项修正后四文件 64/64 通过。
- 最终 `npm test`：1088 个测试文件通过、1 个按既有配置跳过；11016 tests passed、7 skipped、0 failed，退出码 0。
- 最终 `npm run test:artifact-release-smoke`：8 文件、304 tests passed、0 failed，退出码 0。
- `npm run build:packages` 与 `npm run build:client` 均退出 0；客户端构建只有既有的大分块和 splash 非 module 脚本警告。
- darwin-arm64 `npm run build:server` 退出 0；一次性本地 Ed25519 密钥签出的 seed 同时通过 `verify-seed-kit`，manifest 为 `0.1.23`、发布世代 1、来源提交为当前 HEAD。临时私钥目录已确认删除。
- 解包后的 darwin-arm64 server 在隔离 `LINGXI_HOME` 真实启动，`GET /api/server/identity` 返回 HTTP 200、协议 1，包内版本 `0.1.23`。
- 最终三段 `npm run typecheck` 退出 0；最终 lint 退出 0，0 errors、7958 个仓库历史 warning，不借本任务清理范围外警告。
- `npm run release:preflight` 读取全部已获取标签并按 Lingxi 包名过滤：候选 `0.1.23`、历史最高 `0.1.22`、候选世代 1、历史最高世代 0、6 个历史产品发布，退出 0。
- `release-digest.v1.json` 与 `release-digest.v2.json` 仍指向 `0.1.3`；两份对 `v0.1.23` 的校验均退出 1，证明早期发布门禁会真实阻断。解除步骤已写入 `BLOCKED.md`，未手改生成文件、未创建 tag、未发布。
- 最终 `git diff --check`、受控 JSON 解析和待交付文件秘密标记扫描均通过；一次性签名私钥、构建目录与下载取证目录均未进入 Git 待交付清单。

## 2026-08-09：用户授权正式发布续跑

- 用户明确要求继续解除摘要阻塞，随后提交、创建并推送 `v0.1.23` tag、创建 Release 并完成所有发布操作。
- GitHub CLI 已登录 `ItsDalk-Lane`，具备仓库和工作流权限；本地 `main` 与 `origin/main` 同步。
- 当前环境变量未提供摘要生成所需 `OPENAI_API_KEY`，也未直接提供 seed/Train 私钥路径；先检查仓库生成器允许的正规输入方式，不读取或挪用无关凭据。
- 发布前实时依赖复核：生产树 0 critical / 15 high / 18 moderate，完整树 2 critical / 24 high / 19 moderate / 2 low；已确认 critical/high 都有兼容升级路径，先升级并跑完整回归，不能把旧审计数字留作“已知风险”直接发布。
- 已升级可兼容修复的直接依赖并让审计工具更新安全的传递锁定版本：生产树变为 9 moderate、0 critical/high/low；完整树为 9 moderate、1 low、0 critical/high。`npm audit fix` 仍以 1 退出，真实原因是残余中低风险没有安全的原位修复，未使用 `--force`。
- 依赖升级涉及桌面壳、打包、测试、网页构建、服务端、自动更新、Markdown/Mermaid 与网络请求；必须用新的实际安装树重跑闭包生成、全量测试、类型、lint 和关键构建后才允许提交。
- 首次新依赖树全量测试因 `yauzl` 缺失产生跨文件装载失败，类型检查同样退出 2；最小样本证明生产解压模块长期依赖旧打包工具顺带提升的包。已直接声明 `yauzl@3.4.0` 与类型包，解压和模型同步 3 文件、81 项恢复全绿。
- Electron 从 42.3.0 升到 42.8.1 后，shell 表面清单守卫准确发现声明漂移（40 通过、1 失败）；已只同步该受管版本字段，等待全量重跑。
- 修复后重新生成运行闭包：10560 个文件，其中源码图 664、运行资产 11、运行时追踪 9885；开放边界仍为 1 条既有待证据边。shell 表面清单 41/41 通过。
- 第二次全量测试退出 0：1088 文件通过、1 文件既有跳过；11016 tests passed、7 skipped、0 failed。类型检查退出 0；lint 仍为 0 errors、7958 个历史 warning，退出 0。
- Artifact release smoke 8 文件、304 项全绿；发布工作流与 seed 守卫定向运行 2 文件、14 项全绿；`build:packages`、`build:client` 均退出 0。
- darwin-arm64 独立服务端在升级后的真实依赖树上完成构建，签名 seed 校验通过；隔离 `LINGXI_HOME` 启动后身份接口返回 HTTP 200、协议 1、版本 `0.1.23`，一次性私钥和隔离目录随后删除。
- 最终审计取证：生产依赖 9 moderate、0 critical/high/low；完整依赖 9 moderate、1 low、0 critical/high。两条 `npm audit` 都按真实风险退出 1，未把残余风险包装成全绿。
- 本任务 40 个文件已提交为 `cf0be5bc`（`release: repair artifact activation and harden 0.1.23`）；提交前 staged diff 检查和私钥/常见令牌标记扫描通过。
- 提交后 `v0.1.3..HEAD` 摘要来源包生成成功，准确包含 1 个提交；正式摘要生成仍因当前进程没有 `OPENAI_API_KEY` 退出 1。为保持发布门禁真实有效，尚未推送 main、创建 tag 或 Release。
- 用户改选 DeepSeek：目标为 `DEEPSEEK_API_KEY`、`deepseek-v4-flash`、DeepSeek Responses API。已开始核对官方契约并规划最小生成器迁移与安全负向测试；未读取、写入或打印任何真实密钥。
- DeepSeek 官方文档已确认 V4 Flash 和 JSON Output，但当前只找到 Chat Completions/Anthropic 接口，没有找到 Responses API 参考。下一步以无密钥官方端点探测裁决 `/responses` 是否实际存在；若不存在则保持用户选定模型/密钥，使用官方 Chat Completions JSON Output 并继续本地严格校验。
- 无密钥端点探测被统一鉴权网关提前拦截，连故意伪造路径都得到同类结果，不能据此确认 `/responses`；当前环境也没有 `DEEPSEEK_API_KEY` 可做真实探测。继续查官方文档索引，不输出密钥、不使用假成功。
- 已从 DeepSeek 官方站点地图确认 Responses API 指南和 Create Response 参考页面确实存在，用户说明正确；前一步只是搜索索引漏检。实现路线已定为官方 Responses API + `deepseek-v4-flash`。
- 已提取官方契约：目标为 `POST /responses`，V4 Flash 是当前唯一受支持模型，结构化输出支持 JSON Schema。实现将去掉未列入 DeepSeek 契约的 `strict`/`store` 请求字段，并增加 completed 状态检查后再解析输出。
- 迁移测试已先以旧实现运行：14 项中 7 项通过、7 项失败；失败逐一命中旧默认模型、旧密钥/端点、未完成响应、错误体泄露风险和摘要版本错配，证明新断言不是假绿。
- 生成器已改为从 `DEEPSEEK_API_KEY` 取密钥、默认请求 `deepseek-v4-flash` 的 `POST https://api.deepseek.com/responses`；不再发送 DeepSeek 未支持的 `strict`/`store`，也不把远端错误体拼进异常。
- 本地在解析正文前必须看到 `completed`，解析后还会依次执行摘要结构、目标 tag 和目标 version 校验；定向测试已由 7 项失败反转为 14/14 通过。
- 摘要四个相关测试文件合计 43/43 通过，三段类型检查退出 0。首次 lint 的 959 个错误全部落在一个带 ` 2` 后缀的被忽略构建临时副本，不属于源码；已按精确路径核对后清理并重跑，不调整检查规则。
- 清理该构建临时副本后 lint 恢复为 0 errors、7958 个历史 warnings、退出 0；随后全量 `npm test` 退出 0。
- 真实命令在无 `DEEPSEEK_API_KEY` 时以退出码 1 明确失败，且没有创建摘要文件；当前唯一外部阻塞已从 OpenAI 密钥改为 DeepSeek 密钥。
- 首次真实 DeepSeek 调用到达 completed，但响应把思考过程放在最终正文之前；旧提取器拿到首个任意 text 后 JSON 解析失败，摘要未写入。新增同序响应测试先复现 1/14 失败，再把提取范围收窄为明确的 `output_text`。
- reasoning 兼容修复已提交为 `3655c238`；第二次真实 DeepSeek 调用成功写出 `v0.1.23` v1 摘要，并把 v2 史册更新为 4 条、头部 `0.1.23`。
- v1/v2 两条正式校验命令均退出 0。摘要包含三个条目，分别引用 `cf0be5bc`、`1bdb501a`、`3655c238`，人工复核没有发现超出提交事实的内容。
- 摘要落盘后的最终发布预检 PASS：候选 `0.1.23`、历史最高 `0.1.22`、发布世代 `1 > 0`；Artifact release smoke 为 8 文件、304 项通过。
- 最终全量测试退出 0：1088 文件通过、1 文件既有跳过；11021 tests passed、7 skipped、0 failed。新增的 5 项正好来自 DeepSeek 迁移负向/契约覆盖。
- `main` 四个提交已推送到 `1f24a039`，`v0.1.23` annotated tag 也已推送并精确指向该提交；Build run `31297037318` 的预检、摘要、renderer、macOS 双架构与 Linux 构建成功。
- 远端 Ubuntu quality-gate 的全量测试出现 2 文件、8 项失败：正向 Computer Use 测试夹具错误依赖宿主平台，在 Linux 上触发生产代码预期的“不支持”保护。本机 macOS 全绿因此没有提前暴露。
- 修复只约束测试环境：路由正向测试显式传入 darwin，Linux 负向测试继续显式传入 linux；引擎懒加载测试在自身 describe 内临时固定 darwin 并在 afterEach 恢复，不修改产品的平台支持范围。
- 首次 Build 最终结论为 failure：四个平台构建全部成功，quality-gate 失败后 Release、Artifact smoke、Train 按设计全部 skipped，远端没有创建半成品 Release。
- `v0.1.23` tag 已成为公开历史且禁止移动/删除；继续发布版本提升为 `0.1.24`、发布世代提升为 2，使产品版本和 Artifact 发布顺序都严格大于失败 tag。
- `0.1.24` 定向验证：4 文件、24 项通过；release preflight 确认 `0.1.24 > 0.1.23` 且 generation `2 > 1`；Artifact smoke 8 文件、304 项通过。
- 三段类型检查退出 0，lint 为 0 errors / 7958 个历史 warnings；全量测试 1088 文件通过、1 文件既有跳过，11021 tests passed、7 skipped、0 failed。
- 首份 `0.1.24` 模型摘要虽通过结构校验，但人工复核判定“Linux 用户此前无法安装”缺乏证据，已拒绝提交并改用事实说明从最后一个真实 Release `v0.1.3` 重生成。
- 事实约束后的首次重生成返回 incomplete/max_output_tokens，生成器按设计没有落盘；这证明 completed 门禁生效，也说明 4000 额度不足以同时容纳模型推理和完整结构化摘要。
