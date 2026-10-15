# EX10 当前核心/服务交付

本包仅修改私有源树；未改 Main、登记、门禁、原断言、治理或依赖。15 文件净补丁以完整提交 `634c6dc715b1061c77c4e3fcf7acc87b30d55f11` 的逐文件内容为基线；`final-base/` 是准确原文，`candidate/` 是冻结候选。Main 后续 EX08/07/02/11 的改动不包含在本包。新旧文件 SHA、整组 SHA 与补丁 SHA 均见 `final-source-sha.json`。

## 结果与原义务

现役 TS 登记 `studio-mounts.json` 的 storage/local_fs 挂载，沿真实生产 CLI/HTTP 和同一 ResourceIO 内核执行读取、写入、带版本写入、元数据、目录、文本/名称检索、改名、移动、回收、监听、订阅、事件、诊断及释放。原 19 条 assertion/Then 文本保持不变，动作与具体真实证明见 `original-assertion-proof-map.json`。原资源 envelope/ticket/content 保持原身份边界；挂载物化通过原内核和生产模型工具入口，不虚构不存在的 HTTP materialize 路由。

Mount 不是全局本地文件授权：每次实际操作按当前调用主体/会话、当前登记、能力及根目录身份核对；工作区外挂载的成功不让同路径 local-file 获权。监听撤权后释放句柄和订阅，代次保护使旧轮询不能清理新监听。根目录和内部路径使用持有目录句柄的安全操作；移动使用不覆盖的原子跨目录提交，回收的命名空间/元数据准备不跟随链接。失败不假成功，保留已有文件。

三个资源正文提供者使用有界、无链接句柄和读后身份重核。JSON 正文读最大 20 MiB，超限明确 413；二进制 UTF-8 错用明确拒绝。内容 GET/Range 保持大文件能力，以 64 KiB 按需流式读取，HEAD 不读正文。流期间路径变化中止响应，不把外部字节当原文件成功返回。

Mount 原地物化不登记永久 SessionFile ID。只有实际独立 copy 到授权工作区才按旧方式登记新副本，撤销原挂载后副本仍可读；原地引用继续受挂载生命周期管理。保存的是完整 native Mount resource 元数据，下一模型实际输入与存储模型档案同份详情一致。公开 `reference_identities`、显式 fileId、realPath 和 SessionFile 文本标记仍严格做原会话归属校验；仅真实 first-party file/materialize ToolCallCompleted（核对同 Run/SID 的已存 tool_call_started），以及可信同用户模型档案使用 private native 分类。没有用任意 resource 字段、Opaque 或 JSON 拼接绕过旧附件扫描，也没有永久附加授权。

真实待批准 copy 原来仅冻结目标 Write，挂载来源改变后能复制另一来源。本包将真实来源 Read 纳入原网关 prepare/execute 同一规范化检查；执行端只读消费可信 task-local 已绑定范围，并在异步物化返回后再次比较来源，避免确认与真正读取间改根。稳定来源的真实批准复制仍成功；改根后的同一批准返回 scope changed，目标不生成。未改网关鉴权/权限规则。

## 保留的真实反例

- `configured-mount-original-negative.log`：原生产 CLI 读取真实 TS 登记的本地挂载，返回 unsupported 400。
- `resource-read-swap-original-negative.log`：原 local_fs 授权后替换链接，实际读到外部字节。
- `registered-read-swap-original-negative.log`：原 session_file/resource 替换实际源路径，读到 PRIVATE-1。
- `content-stream-swap-original-negative.log`：原真实内容 GET 返回 200 和 PRIVATE-1。
- `mount-materialization-lifetime-original-negative.log`：初版已实现挂载的候选物化登记永久 SFID，撤销挂载后实际后续 extract 仍成功。该项是候选整合缺陷，不声称未实现 mount 的原 Main 已有成功旁路。
- `mount-approval-root-drift-real-before-fix.log`：真实 Run 在 ask 模式等待实际审批，登记 A→B 后批准，真实落盘 FROM-B；原零目标断言 FAIL。修复后相同反例拒绝且零目标，稳定批准落盘 FROM-A。

`mount-approval-root-drift-before-fix.log` 中 503 是注入模型夹具未接 HTTP 审批路由的夹具诊断；真实反例/修后使用网关同一实际 ApprovalService 的 public pending/answer 方法，不据该 503 声称产品失败。该项不声明验证审批 HTTP UI。`protected-case-alias-before-fix.log` 是已有真实保护 PASS 控制，并非失败反例，无对应生产修复；大小写别名在本机实际文件系统受到拒绝。其他保留早期编译/夹具诊断均不计产品反例。

## 验证与真实入口

最终命令及逐项状态见 `verification.json`，联合旧父和新父日志 `final-combined-r6-tests.log`；完整源码窗口见 `validated-runtime-source-sha.json`，15 文件最终验证 SHA 见 `final-validation-source-sha.json`。原 6 套回归测试内容与 634c 完全一致，逐文件证据 `unchanged-regression-source-sha.json`。F08 原投影预算 8 条保持原断言并实际复跑。

`current_cli_registered_mount_http_success_permissions_events_and_disk` 使用当前 `CARGO_BIN_EXE_lingxi-service` 启动真实服务，由现役 TS `upsertStudioMount` 登记合成挂载，再由 Python 真实 HTTP/磁盘交叉核对 16 组动作。父 stdout 给出当次真实 raw 目录；其中 `result.json` 和 `trace.json` 保存请求、响应和实际检查。

`actual_run_mount_materialize_extract_copy_and_revoked_read` 由真实 HTTP execute→RunSupervisor→网关→ResourceTools→存储→下一模型走完整链，6 个正常两轮 Run 和 1 个首轮取消 Run 共 13 次实际 provider port 调用；真实取消状态落 DB，取消后不发下一模型。提供者是实际 TurnProviderPort 替身，不是 HTTP 供应商；该夹具未回报收费 usage，不能据此宣称真实供应商收费验证。审批对照另有两个各两轮真实 Run。旧 Root 资源工具 3 父继续原样运行，包括真实 HTTP 公网 URL 替身与跨会话保护。

## 限制

仅交付 EX10 要求的当前核心/服务份额，不扩大为 Tauri/桌面文件树已验。真实挂载目前只有现役 local_fs 提供者；现役 TS 的远端/studio 名称没有可消费的实际供应商实现及凭证，本包明确 501，不编造配置或声称可达。无登记 provider 的旧构造仍按原语义明确 unsupported。

当前真实文件操作在 macOS 验证。安全 move/trash 的新增 Windows 方法保持显式 Unsupported（既有目录句柄平台边界），Linux 分支未在本机执行；不以 macOS 结果代替全平台。流过程中错误可能发生在响应头发出后，此时明确中断正文，不能撤回已发响应头。回收元数据准备失败保护原文件，但不承诺任意晚期磁盘故障全事务回滚。旧 write 限额沿原 FileSafety，不宣称无限文件写入。

最终冻结验证：联合 67/67 + 流读取 1/1 = 68 PASS；service/adapters strict 均 PASS；新 4 Rust 文件格式检查 PASS；准确基线 apply --check PASS。整 workspace fmt 未在此私树执行，634c 已有未格式化由 Root 统一整理。最终全部 15 个候选文件及完整运行时源窗口 SHA 重核一致。
