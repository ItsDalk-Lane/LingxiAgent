# T04 独立只读审阅（r6）

状态：发现 1 项待修的启动回收问题，不能据此批准 T04 完成。未修改冻结源码，未执行 Cargo。审阅者此前编写的 UI 仅供跨层对照，不计入独立审阅范围。

## 候选与证据绑定

- 候选 `SOURCE_MANIFEST_r6.json`，指纹 `ee770ff89c7262c07dab96c51388b1420385e322d2f6a98bde7e4c7165ba2efe`。
- 独立重算 369 个清单文件的长度、SHA256 和清单指纹，均与审阅时源码相符；当时未发现清单外、未明确排除的改动代码。
- `source-r6.tar.gz` 的 SHA256 为 `2569ed4d1fcfa9a94de9d38053633d72fb32aef501e7f3ac36e485860fab4bf3`；归档校验及全部条目的名称、顺序、长度、内容摘要相符。
- 首次核对时，已完成的 10 份正式结果记录均能核对前后指纹和原始日志 SHA256：browser-contract、browser-flows、contract-types、dependency-boundaries、main-build、ownership、preload-build、ts-roundtrip、typecheck、ui。browser-flows 为 routes 10、tools 13、web 8；UI 为 26 文件、293 项。
- 上述是审阅时的证据快照，不将仍运行或之后失败的门禁计为通过。r5 的真实编译失败保留；其 Phone 校验参数修复属于 r6，不能借用 r5 结果。全阶段最终结论须由最终候选完整门禁决定。

## 发现 T04-IR-01（P2）：启动半成功的未知清理分支丢失所有权

`rust/crates/lingxi-adapters/src/browser/launcher.rs:148`：进程已成功启动，但 `Cdp::new` 因非阻塞设置或读取线程创建失败时，先杀进程组、最多等待 2 秒；若退出仍未确认，则返回 `browser_cleanup_unknown`，局部 `Child` 随返回丢弃，没有交给宿主持有。

`rust/crates/lingxi-adapters/src/browser/host.rs:522`、`:528`、`:581`：`Process` / `Live` 初始化或首个 tab 创建失败，都发生在 `Entry.live` 登记之前。此时主要依赖析构中的 `stop`；该次清理若仍不确定，也不会进入已有的 `pending_cleanup`。辅助浏览器 `:1126`、`:1147` 的目录清理标记在 `Process::launch` 成功返回之前仍为 true，启动清理 Unknown 也可能进入目录清理。

影响：已经启动、退出尚未确认的进程不再有可继续等待和回收的受控对象；其 profile 可能仍被使用，却允许后续重用或清理。已有 `stop_owned` 保留权属的修复没有覆盖这些启动分支。

这是源码中可确认的错误分支；本次没有注入“初始化失败且进程退出超时”的组合，不能声称本机实际遗留过 Chrome。已向主代理和 adapter 作者报告，主代理确认必须修复并派原作者处理。

回归应强制 spawn 后 CDP 初始化失败，以及 Live/首 tab 失败，并令清理结果为 Unknown；断言仍持有 Child/Live、宿主不再派发新执行、profile 保留；只有确认退出后才能清理。需要新的候选指纹及实际验证，不能修改 r6 既有结果。

## 已核对的真实规则

- **急停优先于旧授权**：`storage/browser.rs:132` 每个新人工急停推进 epoch；`:259` 的旧 steer 只在捕获 epoch 仍相同时可以授权。稳定请求回放只返回原收据，不重新停止或解禁。`service/browser.rs:934` 先冻结撤权事实，再调用外部回收；实际交错回归在 `tests/r07_browser_tools.rs:1373`。
- **已登记 Live 的清理失败保留管理**：`adapters/browser/host.rs:228` 失败时保留 `Entry.live` 和 `pending_cleanup` 并封闭新执行；`service/browser.rs:1058` 在 forget 成功之前不删除 known。启动半成功例外见 IR-01。
- **密码隐藏**：`adapters/browser/snapshot.js:35` 不把 password.value 用作名称；`:130` 输出 redacted。`adapters/tests/r07_browser_chromium.rs:683` 同时核正常和紧凑快照无四个密码值，原页面输入仍保留。该规则是快照隐藏，不宣称可信 Evaluate 或网页自行显示出的文字也经过密码识别。
- **读取只授权真实附件及具体版本**：`service/resource_tools.rs:76` 先按真实会话查附件并使用返回的 canonical fileId，`:283` 物化后以实际主体、会话、路径和 Snapshot.stamp 授权；`service/resourceaccess.rs:346`、`:449` 在读取前重核该版本，不开放整个缓存目录，不授予写能力。正式 read 通过原资源权限检查。
- **保留宿主目录声明**：`service/lib.rs:1618` 仅在 read 缺席时补正式只读工具；非空的已注入工具面保留原 catalog 策略。`service/filetools.rs:2966` 没有附带开放 write/edit/exec。
- **无父 Subagent 兼容需要真实预登记**：`service/runs.rs:1330`、`:1434` 先核同一 SessionAdmission 与正式许可；`service/task_supervisor.rs:507` 要求 principal、session、agent、授权和 gate 完整匹配。`phone_run_authority.rs:30` 仅对无父、无 sourceMessage/cause 的正式 Subagent 作兼容；有后台来源仍逐级追溯。正式 `phone_tools.rs:67` 明确传 false。`tests/r04_t02_tool_gateway.rs:2092` 未预登记时不得创建 Run；`:2121` 缺真实父链时零工具派发。
- **冷恢复和删除**：`service/browser.rs:116` 启动只恢复同节点、同实际 owner 的冷元数据，不打开旧网址；`adapters/browser/host.rs:999` 删除先退休 scope，停止确认、等待启动收口后才清 profile/download，成功后移除条目。

## 限制与交接

本次不替代全部门禁，也不覆盖其他操作系统、正式发布包或真实账号。T05 未注册开发文件仍按清单明确排除，不能作为办公功能完成证据。IR-01 关闭前保持未批准；交接后释放并行槽，等待 T05 共享读写边界解冻。
