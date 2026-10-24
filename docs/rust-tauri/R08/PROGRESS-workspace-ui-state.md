# R08-T01 workspace-ui-state 叶子完成记录

执行者：teammate `ws-ui-state`。日期：2026-10-22（Asia/Shanghai）。
范围：现役 Electron 客户端 `GET/PUT /api/preferences/workspace-ui-state` 到 Rust 服务
`/lingxi/v1/preferences/workspace-ui-state` 的忠实移植 + 前端转发 + 全量验证证据。

## 实现要点

- 新建 `rust/crates/lingxi-service/src/workspace_ui_state.rs`（730 行），完整移植现役
  `server/http/routes/workspace-ui-state.ts`、`shared/workspace-ui-state.ts`、
  `shared/preview-reading-position.ts`、`shared/workspace-history.ts:3-11`、
  `shared/workspace-persistence-gc.ts` 的全部规范化与 GC 语义：
  - 全量规范化链：workspace 路径（盘符/反斜杠/尾斜杠）、surface（electron/pwa，
    缺省/非法回落 electron）、相对路径（去 `.`/`..`/盘符逃逸、去重、上限 256）、
    desk/jian/rightWorkspaceTab 字段、openTabs 过滤重排 + activeTabId 回落、
    previewTabs 按 id 去重上限 32、readingPosition（scrollTop 必填 clamp 1e7，
    scrollLeft/scrollHeight/clientHeight/anchorId/anchorText/contentHash 仅
    truthy 保留，ratio/updatedAt 定义即保留）、record.updatedAt = max(各 surface)、
    旧版扁平记录（无 surfaces）自动物化到 electron 桶、>50 workspace 按
    updatedAt 降序裁剪到 50。
  - GET 必经缺失目录 GC：`fs::metadata`（跟随符号链接），ENOENT/ENOTDIR 裁剪并
    仅在发生裁剪时落盘，其他错误（EACCES 等）保留；无状态 GET 不创建
    preferences.json。PUT 不做 GC（忠实：`setWorkspaceUiState` 无 gc 调用）。
  - JS 数值语义：`js_num`（整数 ≤2^53-1 发 i64，否则 f64）、JS trim 空白集、
    UTF-16 码元截断向下取整到 char 边界。
- `rust/crates/lingxi-service/src/lib.rs`：注册路由
  `/lingxi/v1/preferences/workspace-ui-state`（GET+PUT）。
- `rust/crates/lingxi-service/src/auth.rs`：RoutePolicy 新臂，逐字对齐现役
  `server/http/route-security.ts:74-78`——GET/HEAD→Scope("files.read")，
  PUT→Scope("files.write")，其他动词→LocalOnly。
- 前端 `desktop/src/react/hooks/use-hana-fetch.ts`：serviceRoute 正则表新增
  `/api/preferences/workspace-ui-state`（仅路径前缀映射，query 与 body 逐字节
  透传，与 appearance 同形）。

## 端口决策（与现役语义的一致性说明）

1. 400 文案逐字保留：`workspace must be a non-empty path` /
   `workspace UI surface is invalid` / `invalid JSON body`；体形状对齐现役
   safeJson 口径（不可解析/空体→按 `{}` 走到 workspace 校验 400；null/标量→
   invalid JSON body；数组→无字段对象→workspace 校验 400），且 400 均不落盘。
2. 存储层错误（preferences.json 损坏、IO 失败）→ 响亮 500 平铺 `{"error": msg}`，
   GET/PUT 一致。这是对现役 PUT catch→400 的**有意偏离**，沿用 appearance 叶子
   已确立的"存储故障不伪装成客户端错误"先例（错误要么抛要么显式标注，禁止静默
   降级）。
3. GC 一并移植（现役 GET 必经 `pruneMissingWorkspaceUiState`），非新增行为；
   "changed=pruned" 等价性保证未变化时不落盘。
4. `serde_json::Map`（BTreeMap）键序注意事项：仅在 >50 个 workspace 且
   updatedAt 完全相等时裁剪成员选择与 JS 稳定序不同；模块内已文档化，测试均用
   互异 updatedAt，不影响任何已验证行为。
5. 零新增第三方依赖；时钟走 `state.clock()`（测试 ManualClock 固定
   1_800_000_000_000，验证 now 回落与 record.updatedAt=max）。

## 验证证据与结果计数

证据根：`artifacts/rust-tauri/R08/`。

| 证据目录 | 命令 | 结果 |
| --- | --- | --- |
| `t01-ws-ui-check-01` | `cargo check -p lingxi-service` | exit 0 |
| `t01-ws-ui-suite-01` | `r07_stage_inputs.py --run-suite r07_t08_interface_preferences`（SUITES 子目录含 result.json/output.log） | exit 0；**8 passed / 0 failed**（含新增 2 测试） |
| `t01-ws-ui-unit-01` | `npx vitest run desktop/src/react/__tests__/hooks/use-hana-fetch.test.ts` | exit 0；**103 passed / 103** |
| `t01-ws-ui-typecheck-01` | `npm run typecheck`（root+node+test 三套 tsc） | exit 0 |
| `t01-ws-ui-auth-01` / `t01-ws-ui-auth-02` | `cargo test -p lingxi-service --test auth_matrix` | exit 101；**环境性既有失败，非本叶子引入**（见下） |

新增服务测试（`rust/crates/lingxi-service/tests/r07_t08_interface_preferences.rs`，
套件 6→8 测试）：

- `actual_http_workspace_ui_state_round_trip_normalize_surface_scope_and_same_home_restart`：
  初始 `{"state":null}` 且不落盘；空 workspace/非法 surface/缺 workspace 400 同文案；
  无凭证 401；files.read 仅 GET（PUT 403）、files.write 仅 PUT（GET 403）；全字段
  PUT 规范化逐字段断言（路径去逃逸/去重、openTabs 重排、activeTabId 回落、
  scrollTop clamp、truthy 字段、未知字段丢弃、显式 updatedAt）；surface 分桶 +
  now 回落 + record.updatedAt=max；previewTabs 上限 32 + openTabs 缺省回填首 tab +
  严格布尔 + 非法枚举回落；旧版扁平记录迁移；5 种体形状 400 且断言文件字节不变；
  同 home 重启后读回。
- `actual_http_workspace_ui_state_gc_prunes_missing_and_trims_over_fifty`：
  PUT 不 GC（missing/ENOTDIR 先落盘）→ GET 触发裁剪并落盘；EACCES（0o000 父目录）
  保留（root/非 root 两环境断言一致）；52 个播种 workspace + 1 次 PUT 触发降序裁剪到
  50（断言保留/剔除成员）；被裁剪但目录存在的 workspace GET→`{"state":null}`；
  GC 未变化时文件字节不变。

前端测试：路径表新增带 query 表项、`/api/preferences/workspace-ui-statex` 前缀
不误吞、GET/PUT 逐字节转发到 `http://127.0.0.1:3210/lingxi/v1/preferences/workspace-ui-state`
（含 query 保留）。

## auth_matrix 失败说明（环境性、既有、与本叶子无关）

`--test auth_matrix` 两次运行（-01/-02）均以同一原因失败：25 个测试中 24 个在
harness `ServiceState::bootstrap_with_limits` 阶段 panic（auth_matrix.rs:94），
错误 `Storage(InvalidRequest { detail: "MCP 原始只读目录不可用" })`
（mcp_settings.rs:541，MCP 设置存储层 bootstrap 前置）；唯一通过的是不启动服务的
配置层测试 `a05_loopback_mode_refuses_non_loopback_bind_at_config_layer`。

既有性证明：`artifacts/rust-tauri/R08/t01-permission-mode-auth-matrix-01`
（13:15，另一叶子证据，candidate.diff 零处提及 workspace_ui_state，早于本模块创建
时间 14:32）呈现完全相同的失败签名（1 passed / 24 failed，同一 bootstrap 错误）。
本叶子 diff 未触碰 mcp_settings.rs、auth_matrix.rs 或任何 bootstrap 路径
（`git status --porcelain` 为空）。失败发生在路由注册与鉴权策略生效之前，与新增
路由/auth 臂无因果关系。按"3 次同因失败即停并上报"规则，-01/-02 两次同因后停止
重跑。本路由的 scope 分类已由 r07_t08_interface_preferences 套件内的真实 bootstrap
集成测试（401/403/files.read/files.write 全断言，8/8 绿）覆盖验证。

## 变更文件清单

- `rust/crates/lingxi-service/src/workspace_ui_state.rs`（新增，730 行）
- `rust/crates/lingxi-service/src/lib.rs`（模块声明 + 路由注册）
- `rust/crates/lingxi-service/src/auth.rs`（RoutePolicy 新臂）
- `rust/crates/lingxi-service/tests/r07_t08_interface_preferences.rs`（+2 测试，788→1303 行）
- `desktop/src/react/hooks/use-hana-fetch.ts`（+1 正则表项）
- `desktop/src/react/__tests__/hooks/use-hana-fetch.test.ts`（+3 断言组）

未运行 `r08_t01_api_binding.mjs`（绑定表由 lead 统一再生成）；未提交任何更改。
与现役契约的偏差：仅上述"存储错误 500"一项有意决策（appearance 先例），其余为零偏差。
