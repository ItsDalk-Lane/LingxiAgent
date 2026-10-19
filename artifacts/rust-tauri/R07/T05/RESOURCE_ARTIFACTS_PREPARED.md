# T05 办公产物读写准备交接

状态：`PREPARED_NOT_REGISTERED_NOT_COMPILED`。未运行 Cargo，未应用共享文件补丁，不作为办公能力已完成或 T04 通过的证据。

## 新文件与待应用补丁

- `rust/crates/lingxi-service/src/resource_artifacts.rs`：预期由 `resourceio.rs` 作子模块加载，包含三个 crate-only 方法、闭合持久版本事实、3 项待跑单元回归。T04 候选需明确排除本文件。
- `file_safety-artifact-budget.patch`：仅准备；原 `file_safety.rs` 字节未改。新增私有 Text/Artifact 预算，普通创建、替换、来源证明仍为8MiB；`read_bounded` 仍最多20MiB；办公入口独享50MiB，来源证明、提交重读、交换原件核验、冲突回换、新对象核验全部使用同一预算。补丁含4项待跑回归。
- `file_safety-artifact-budget.candidate.rs`：补丁应用后的预览源，不参与生产编译。准确基线和补丁摘要在 `FILE_SAFETY_PATCH_STATUS.json`，应用前须核基线。
- `resourceio-artifacts-module.patch`：仅准备，将新文件作为子模块并 reexport `ArtifactSnapshot` / `ArtifactWritePreflight`，不新增 HTTP 能力。
- `office_tools.rs` 属未注册 T05 文件，已直接改用新入口；原普通文字/JSON接口没有改动。

## 接口及行为

```rust
read_artifact_snapshot(&self, input: &Value, ctx: &OpContext)
    -> Result<ArtifactSnapshot, ResourceIoError>
prepare_artifact_write(&self, input: &Value, overwrite: bool, ctx: &OpContext)
    -> Result<ArtifactWritePreflight, ResourceIoError>
publish_artifact(&self, preflight: &ArtifactWritePreflight, bytes: Vec<u8>, ctx: &OpContext)
    -> Result<Value, ResourceIoError>
```

读取保留实际阻塞许可、取消范围和会话载荷租约；本机和挂载沿正式资源授权、固定句柄及完整版本检查。SessionFile 与 `res_sf_` 只在真实当前会话解析；旧 alias 必须替换为该会话返回的 canonical fileId 并显式带 sid，再交解析器，不能落回全局查原会话。URL、其他未接 provider 明确拒绝。返回 bytes 和 `meta.artifactStamp` 来自同一快照，没有读取后另 stat 配错版本。

预检不建目录、不建临时文件、不写 witness/recovery、不发事件。它固定 typed ref、实际路径、主体/会话、overwrite、完整目标 stamp、最近现存父目录 dev/inode、缺失目录段及挂载根身份。存在目标用 SHA256 与原身份；缺失目标只能在最终发布时创建。中途外部创建了原来缺失的父目录也冲突，不默用新对象。

发布不另收新路径或 overwrite；重新检查当前许可、取消、typed mount Write、原挂载根、父目录与原文件完整 stamp。缺失父目录逐级在固定句柄内独占新建；再沿同一 FileSafety 原子提交与恢复机制写入。写后只有同一版本的真实来源证明确认，才返回成功并发资源变化事件；写后资源映射或历史事实失败明确 Unknown，不报零副作用失败。原二进制/文字历史限制不扩大。

## 后台生成的接线约束

`ArtifactWritePreflight` 字段私有，无 Deserialize。`persistent_facts()` 只保存版本事实；`from_trusted_persistent_facts()` 仅供真实后台任务数据库重读原值，不能转发 HTTP/模型自报 facts。恢复不会重新采纳现版本，最终发布仍核当前权限。

Office 新增 `apply_cover_frozen(args, context, operation, &preflight)`。真实 MediaRun 必须在付费前、以正式 Run 的实际 OpContext 冻结目标并把原事实持久化；完成/重启后仍使用同一 scope 的原预检。预检绑定主体和会话，不能把 HTTP 的空 sid 事实直接当成另一个后台 sid 的权限。若跨入口交接，需在实际 Run 付费前核对原目标和原完整版本，再保存该 Run 的正式事实；不能在付费后重预检替换原事实。HTTP generation 的正式 MediaRun 接线仍由主代理负责，当前没有伪造 running。

Office PDF 已在渲染前冻结 outputPath，渲染后按该对象发布并登记实际文件；封面先核原 Markdown 完整 stamp，图片走窄产物发布，Markdown 输入与更新仍明确不超过8MiB。新内部读取允许50MiB，不扩大普通 JSON20MiB / 文件文字写8MiB。

## 只完成的静态检查

- 新文件、Office 未注册文件及 FileSafety 预览经 `rustfmt --edition 2021` 退出0，仅证明语法可解析和格式稳定。
- 两补丁 `git apply --check` 退出0；它们没有应用。
- 三接口、7项新单元回归和全部 Rust Office 集成均尚未编译或运行。后续必须补真实 local/mount/SessionFile50MiB、旧 alias、实时撤权、取消、无副作用预检、强版本竞争和恢复回归，不能用当前静态检查替代。

后续目标命令（主代理确认接线并统一 Cargo 后）：

```sh
cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib resourceio::artifacts::tests
cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib file_safety::artifact_budget_tests
```
