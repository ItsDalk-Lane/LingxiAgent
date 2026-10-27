# R08-T01 env-deps 端口完成记录（teammate env-deps）

## 范围

现役 `GET /api/system/env-deps` 与 `POST /api/system/env-deps/refresh`
（server/routes/env-deps.ts + server/lib/env-deps/{detect,registry}.ts）到 Rust
候选的忠实移植。无 commit / 无 push，全部改动未提交。

## 改动文件

| 文件 | 改动 |
| --- | --- |
| `rust/crates/lingxi-service/src/env_deps.rs` | 新建（约 760 行）：13 项注册表、探测原语、受管目录查找、信号扫描、缓存、路由 |
| `rust/crates/lingxi-service/src/lib.rs` | `pub mod env_deps;` + 两条路由（GET env-deps / POST env-deps/refresh；refresh 的 GET 保持 404 与现役一致） |
| `rust/crates/lingxi-service/src/auth.rs` | 两条路径 LocalOnly 分类（见移植决策 1） |
| `desktop/src/react/hooks/use-hana-fetch.ts` | 前端映射 `/api/system/env-deps(?:/refresh)?` → `/lingxi/v1/...`，query 逐字节保留 |
| `desktop/src/react/__tests__/hooks/use-hana-fetch.test.ts` | 映射用例：同构路由表 +2、负例 env-depsx、POST refresh 参数保留、Node 连接保持现役路径 |
| `rust/crates/lingxi-service/tests/r08_t01_env_deps.rs` | 新建集成套件（单测试函数六相位，见下） |

## 移植决策（与现役差异逐条登记）

1. **授权**：现役 route-security.ts 无 env-deps 显式条目 → 全部动词落
   `/api/*` STUDIO_OWNER 兜底（studio owner 会话凭证）；候选无该会话概念，
   按取最严原则分类 **LocalOnly**（仅本机 token）。语义比现役更严是刻意
   收敛：该面会枚举本机二进制路径，远程可达无正当消费者。
2. **findBundledBin 恒为 None**：现役 bundled 查找依赖 Electron 打包资源
   （lib/bundled-bins.ts），独立 Rust 服务无此拓扑 → 只有受管目录 + PATH
   两级探测。属拓扑裁决，非静默降级。
3. **受管目录根**：现役零参 resolveLingxiHome() 恒 `~/.lingxi`；候选用
   `state.config().data_home`（默认部署两者重合；测试可注入隔离 home，
   数据安全优先）。受管子路径仍为 `runtime/pi-sdk/bin`。
4. **探测并发 → 顺序**：现役 Promise.all 并发 13 项（总时长被 3s 上限
   封顶）；候选顺序探测（最坏 13×3s，实测整轮约亚秒）。经 lead 批准，
   仅影响延迟，不影响报告内容。
5. **进程超时自实现**：std::process::Command 无内建超时 → spawn + 双读
   线程 + try_wait 轮询（20ms）+ 到期 kill。3s / 64KB（probeBinPath 16KB
   现役差异：现役 maxBuffer 截断不报错，候选读全量再截取，语义等价于
   extractVersion 只看首行）。
6. **note 字段不产出**：现役 note 在构造响应时已被丢弃（missing/非零
   退出/超时均不落 JSON），候选直接不建该字段。错误分类（spawn 失败
   →not_found，超时→probe_timeout）为内部路径，JSON 输出与现役逐键一致。
7. **时钟**：checkedAt/durationMs/缓存 TTL 用真实系统时钟（SystemTime/
   chrono rfc3339 ms Z），不用注入的 ManualClock——与现役 Date.now() 一致。
8. **查询解析**：手写首现优先解析器（`+`→空格、`%XX` 十六进制、非法
   序列原样保留、lossy UTF-8），对齐 Hono query() 语义；不用 axum
   Query<T>（重复键报错、语义不符）。agentId 空串→null 不 trim，与现役
   `query("agentId") || null` 一致。
9. **根解析**：agentId 缺省 → 现役 listAgents()[0] → 候选
   `ordered_live_agents().ok().and_then(first)`；显式/默认根经
   `desk_files::selected_agent_desk_roots`（explicit + home_cwd 回落服务
   workspace_root = 现役默认工作区 OH-WorkSpace 的候选对应物），去重保序。
   未知 agentId 不报错（回落默认工作区），零助手 → 根空 → 项目感知关闭。
10. **缓存**：单槽（key=roots.join("|")，60s TTL，命中需 !force 且键同且
    未过期），**force 也写槽**——与现役 detect.ts :215-217 一致。进程内
    Mutex，中毒经 into_inner 恢复。

## 验证证据（均 r08_run.py 包装，exitCode 0，sourceBindingsStable）

| 证据目录 | 命令 | 结果 |
| --- | --- | --- |
| `artifacts/rust-tauri/R08/t01-env-deps-check-01` | cargo check -p lingxi-service --tests | 通过（含全部队友目标，0 error） |
| `artifacts/rust-tauri/R08/t01-env-deps-suite-01`（嵌套 SUITES/r08_t01_env_deps） | r07_stage_inputs.py --run-suite r08_t01_env_deps | 1 passed / 0 failed（六相位单测试，6.2s） |
| `artifacts/rust-tauri/R08/t01-env-deps-unit-01` | npx vitest run …/use-hana-fetch.test.ts | 103 passed (103) |
| `artifacts/rust-tauri/R08/t01-env-deps-typecheck-01` | npm run typecheck | 通过 |
| `artifacts/rust-tauri/R08/t01-env-deps-auth-01`（嵌套 SUITES/auth_matrix） | r07_stage_inputs.py --run-suite auth_matrix | 25 passed / 0 failed（回归） |

## 套件相位（r08_t01_env_deps.rs，真实服务+真实凭证+真实探测）

1. 授权门：无凭证 401×2；scoped(settings.read) 403×2（日志实证
   `reason=local_owner_required`）。
2. 报告契约：summary.total=13；id 顺序与注册表逐项一致；status 仅
   installed/missing；missing 无 version/path/managed 键；installed 带布尔
   managed；零助手无 agentId → 全部 neededByProject=false、
   projectMissing=[]。
3. 未知 agentId → 200；根回落服务 workspace_root：gopls=true（go.mod）、
   python3=true（.github 跳过例外）、node=false（node_modules/.git 内
   package.json 必须被跳过）。
4. 默认第一助手：desk.home_folder=ws2（仅 Cargo.toml）→ 无 agentId 时
   rust_analyzer=true 且 gopls=false（根真实切换）；显式 agentId=alpha
   命中同缓存槽（checkedAt 相同）。
5. 受管目录（cfg unix）：平铺 rg 桩 → installed+managed:true+
   version "14.1.1"+绝对路径；一层子目录 fd 桩退出码非零 → 仍
   installed+managed:true 且无 version（现役忽略 probe ok）。
6. 缓存生命周期（真实时钟）：同键 GET 命中；refresh=1 → 新 checkedAt 且
   覆写槽；POST /refresh → 新 checkedAt 且覆写槽；每次 sleep 50ms 保证
   毫秒分辨率可区分。

单测试函数设计：进程内缓存为全局单槽 Mutex，多 #[tokio::test] 并行会互相
驱逐缓存键导致 checkedAt 断言抖动，故六相位串行于一个函数。

## 备注

- 未运行 `r08_t01_api_binding.mjs`（lead 统一再生成绑定表）；未触碰
  PROGRESS.md 总账。
- 迭代期间曾见队友目标 `r08_t01_a01_a02_ws.rs` 编译错误（非本任务文件），
  正式 check 运行前已被其修复，全量 --tests 绿。
- 前端消费者 EnvDepsTab.tsx 消费的字段（checkedAt / deps[].status /
  version / path / installHint / neededByProject / label / id）与端口输出
  逐键核对一致。
