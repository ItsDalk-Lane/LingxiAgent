# H1 file-url-asset failure (v3.1)

- 现象：`<img src=convertFileSrc('/…/home-native-e2e/t07-asset-probe.png')>` → `img-onerror`（URL `asset://localhost/%2Fprivate%2Ftmp%2F…%2Fhome-native-e2e%2Ft07-asset-probe.png`）。
- 已核对：文件存在（70B，有效 1×1 PNG）；隔离 LINGXI_HOME=该 home；host.rs setup 在窗口创建前 `asset_protocol_scope().allow_directory(home, true)` 且无错误日志；base/release/debug-generated 三份 conf 的 CSP 与 assetProtocol 表面正确；Cargo `protocol-asset` 已启用；G1 exe embedded CSP 含 `asset:`；`$HOME/.lingxi/**`、`$TEMP/**` 等 scope 字符串存在于 embedded 数据中（Tauri embedded config 为序列化结构，不能凭 strings 判定 enable）。
- 待 T06：运行期打印 `app.config().app.security.asset_protocol` 与 `asset_protocol_scope` 实际 roots；对目标文件试 `allow_file`；确认 debug 形态 asset handler 注册；必要时重出 G1 evidence（`--no-renderer-build`）。
- 详见 `h1-asset-diagnosis.json`；preflight/post ps 均 0；H2/H3 hold。
