# R07-T05 办公文件窄适配设计证据

状态：三接口已准备在未注册的 `resource_artifacts.rs`；Office 未注册源码已改用它们。FileSafety 仍仅有待应用补丁，未修改 T04 冻结源码、未运行 Cargo，不能作为能力通过证据。准备清单见 `artifacts/rust-tauri/R07/T05/RESOURCE_ARTIFACTS_PREPARED.md`。

## 已确认的现行边界

| 源码 | 实际行为及本次含义 |
| --- | --- |
| `resourceio.rs:1073`、`:3020` | 文件快照已经持真实阻塞许可、取消范围、会话载荷租约，读取前后授权并核固定文件/目录身份；现限20MiB。应复用此链，不另开路径读取。 |
| `file_safety.rs:30`、`:63` | 普通可恢复写8MiB；`read_bounded`另限制最多20MiB。直接扩大常量会改变原文字/JSON能力，本次不能这样处理。 |
| `file_safety.rs:202`、`:347`、`:384`、`:396`、`:428` | 大文件发布不只需改入口上限：成功来源证明、交换前核验、被移出原文件读取、冲突回换识别与回换对象校验目前均受8MiB限制，必须按同一专用预算贯穿。 |
| `resourceio.rs:2806`、`:2875`；`resource_mounts.rs:175`、`:218` | typed mount引用带独立 Write能力、固定根目录句柄和实时登记/取消检查。不能物化后改传普通绝对路径，否则会丢失挂载约束。 |
| `resourceio.rs:3146` | 现版本写只比旧接口要求的mtime/size等字段，然后对提交时快照作原子保护。办公付费前冻结应使用完整身份/内容stamp，不能只使用弱mtime+size。 |
| `resourceio.rs:2572`；`filetools.rs:647` | `open_resource_parent(...,true)`与`child_directory(...,true)`会创建目录，不能用于“无副作用预检”。新父目录的创建只能发生于实际发布阶段。 |
| `resourceio.rs:1529`、`:1654`、`:1777` | session-file/resource普通提供者有全局ID解析，原HTTP鉴权在外层；办公新读口必须自带真实会话归属核验，不能仅复用global lookup。 |
| `file_safety.rs:299`及后续 | 交换原文件前写持久恢复记录；原文件被移至保留对象；取消/竞争时尽可能回换，不能确认时保留双方并返回Unknown。不得删掉此边界换成普通rename覆盖。 |
| `resourceio.rs:1434`；`filehistory.rs:57`、`:408` | 普通文字历史有独立5MiB/文本类型规则。产物发布仍发正式资源变化事件并保留FileSafety恢复线索，不借本次扩大文字历史或宣称大二进制已有同等文字历史快照。 |

## 最小接口

全部只供Rust宿主/服务内部调用，不新增HTTP路由、renderer输入、模型参数或可自行选择的预算字段。

```rust
pub(crate) struct ArtifactSnapshot {
    pub file_path: PathBuf,
    pub bytes: Vec<u8>,
    pub meta: Value,
}

pub(crate) async fn read_artifact_snapshot(
    &self,
    input: &Value,
    ctx: &OpContext,
) -> Result<ArtifactSnapshot, ResourceIoError>;

pub(crate) async fn prepare_artifact_write(
    &self,
    input: &Value,
    overwrite: bool,
    ctx: &OpContext,
) -> Result<ArtifactWritePreflight, ResourceIoError>;

pub(crate) async fn publish_artifact(
    &self,
    preflight: &ArtifactWritePreflight,
    bytes: Vec<u8>,
    ctx: &OpContext,
) -> Result<Value, ResourceIoError>;
```

`ArtifactWritePreflight`字段私有，至少固定：归一化的原始typed引用/资源键、实际目标路径、`overwrite`决策、当前目标存在状态、完整文件身份和SHA256 stamp、父目录或最近存在祖先的身份、尚缺的规范目录段。不得从模型或HTTP JSON直接反序列化成预检对象。

发布入口只接已固定对象，不再另外接可变path或overwrite。预检仅记录版本事实，**不授予资源权限**。发布仍使用当前真实OpContext重新授权，并核对与冻结对象的资源及身份一致。

后台任务需要跨进程保存预检事实时，单独保存闭合版本事实（typed引用、完整expected stamp、祖先身份与缺失目录段），从真实持久任务重读；恢复后仍重新授权和核版本。不得重新预检后把新版本当成旧任务原本确认的版本，也不得保存任意指针、目录句柄或“允许写入”的布尔授权。

## 读口实施约束

1. 固定专用50MiB上限。现有`read`、`read_filesystem_snapshot`、`registered_json_snapshot`和JSON输出限制继续20MiB，普通`Snapshot::read`继续8MiB。
2. local/mount沿原`blocking_permit`、`operation_cancel`、payload lease、`filesystem(...,"read",...)`，句柄读前检查实际大小，分配后最多读取上限+1字节，读后核授权、路径/目录身份和内容stamp。
3. session-file与`res_sf_`只解析当前真实`ctx.session_id`拥有且available的登记文件；明示其他sid或全局ID属于其他会话时403。**get_file返回本会话实际登记后，应固定它的实际fileId及显式当前sessionId再读取**，不能把无sid旧alias再次交给global resolver，否则fork可能校验本会话却读取原会话载荷。全过程持载荷租约，读后再核登记路径、状态和对应身份，不能从登记之外的路径取内容。普通URL与未接线资源提供者继续明确拒绝。
4. 返回元数据的版本来自同一固定快照，不在读取后另stat一次把新版本配给旧字节。读取不会产生来源证明、检查点、资源变化事件或历史条目。

## 无副作用预检与最终发布

1. 先取得当前真实许可/取消范围；保留typed引用，通过`filesystem(...,"write",...)`及`WorkspacePlane::authorize(ResourceOp::Write)`核当前权限。服务保护目录、目录目标、链接/非普通文件均拒绝。
2. 已存在目标：`overwrite=false`立即冲突；允许覆盖时固定≤50MiB真实快照的完整stamp（含内容SHA256及平台可用身份），丢弃预检正文后仅保存版本事实，避免长时间占用旧文件字节。
3. 未存在目标：只读逐级打开最近存在祖先并记录身份、规范缺失目录段；不mkdir、不建临时文件、不写来源或恢复记录。不假定未来可以写入，也不把FileSafety写一次作为探测。
4. 发布前重新核当前真实Run/会话权限与取消、typed mount Write能力和根身份；资源映射必须与原预检相同。现有目标完整stamp变化、原来缺失目标已出现、祖先被替换或链接化时409，不能覆盖或转到新路径。
5. 确认后才经DirFd逐级创建缺失父目录，每步复核当前授权和祖先身份；最终仍使用持有目录句柄。继承现有平台真实能力，缺少安全句柄能力时明确拒绝，不能回落为裸`create_dir_all`冒称等价。
6. FileSafety内部增加私有文字/产物预算分支：原create/replace/record_written仍走8MiB；crate-only产物入口走50MiB。使用同一原子提交、fsync、来源证明、保留原文件、竞争回换和持久Unknown语义，所有内部重读/回换也使用对应预算。
7. 50MiB新产物不能覆盖超过50MiB的原文件后才发现无法校验或回换；预检与发布都须在任何提交前拒绝此情况。
8. 写入错误按原`WriteError`变体映射：Conflict明确409；Failed为已知失败；Unknown使用稳定`artifact_write_unknown`错误码并保留真实恢复记录，不仅拼字符串再当普通400。
9. 只有真实发布确认后才发资源变化事件、调用适用历史捕获并登记SessionFile。登记或后置事实失败需如实说明“文件已写入，交付未确认”，不能称零副作用失败或制造文件卡。

## Office最小接线

- `OfficeApi::read_resource`改用专用快照；宿主正式ResourceIO仍承担归属/租约，Office当前额外会话校验保留。
- 封面：预检Markdown与稳定附件目标，图片按25MiB原契约限制；附件确认发布后，Markdown仍走普通≤8MiB版本写，并复核原读取版本。部分完成与Unknown保持准确。
- PDF：渲染前冻结指定输出目标；渲染后publish再次核原预检版本。默认托管SessionFile沿现有真实登记，不创建任意目录权限。
- HTTP生成：媒体后台Run付费前检查真实目标Write权限与冻结版本；最终应用时重核。不能为了通过探测把原Markdown原样写回。
- worker内部请求已经72MiB、输入50MiB、输出16MiB；实际Node使用384MiB V8堆、统一超时/取消/终止回收。普通工具参数和HTTP1MiB预算不变，上传封面专用路由另按原25MiB字节预算承载base64。

## 必要验证

- 50MiB local、mount及已上传会话文件实际读取成功；+1字节拒绝；原JSON20MiB和普通文字写8MiB上限不变；其他会话文件、只读/停用挂载均拒绝。fork旧fileId alias必须读本会话真实登记载荷，不能回到原会话文件。
- 预检前后文件内容、目录树、来源证明、恢复记录、资源事件与历史条目不变；缺失父目录仍不创建。
- 预检后目标修改、相同mtime/size但内容改变、同名目录替换、符号链接换入、挂载根更换、撤销Write、真实Run取消，均不能发布旧任务产物。
- 新建和替换大产物实际字节/SHA与SessionFile内容一致；大原文件覆盖、竞争回换和故障注入保全旧内容；交换后故障明确Unknown并保留恢复对象。
- 已知未写入错误没有假文件卡；实际写入后登记失败不得伪造已交付；重启Unknown不自动再覆盖。
- 已有R06适用文件保护/恢复/挂载回归与T05真实Run集成，由主代理统一Cargo调度。这里未执行、不预填PASS。
