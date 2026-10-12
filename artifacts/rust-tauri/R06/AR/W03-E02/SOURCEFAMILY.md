# W03 实例02 同族范围

完整原始命中保存在 consumer-scan-raw.json；命中条数不等于义务通过数。下列各族保留原合同和实际消费者。

## 共享准入

按真实 canonical SQLite 路径共享 Arc；多 SID 排序一次获得；宿主身份绑定优先于 Busy；独立 home 互不拖住；生产跨进程排他由 InstanceGuard 承担。

rust/crates/lingxi-kernel/src/session_admission.rs, rust/crates/lingxi-adapters/src/storage/run_store.rs, rust/crates/lingxi-kernel/src/ports.rs, rust/crates/lingxi-service/src/sessions.rs, rust/crates/lingxi-service/src/lib.rs

## 会话变更

fork/retry/rewind/create/archive/restore/delete/cleanup 持有操作许可直到收据与关键状态落库；原 rename/pin/memory 独立元数据不被网络等待阻塞。

rust/crates/lingxi-service/src/session_tree.rs, rust/crates/lingxi-service/src/session_admin.rs, rust/crates/lingxi-adapters/src/storage/session_tree.rs, rust/crates/lingxi-adapters/src/storage/session_admin.rs

## 日志与事件

追加 v10；v1–9 SQL 原字节不变；关键状态/事件/完成收据同事务；未知文件效果不自动重做；已删除 SID 原 journal 可授权查询且不得重新占用。

rust/crates/lingxi-adapters/src/storage/session_operations.rs, rust/crates/lingxi-adapters/src/storage/migrations.rs, rust/crates/lingxi-service/src/auth.rs

## 运行与私有凭据

public/borrowed/owned/durable background 实际入口校验 Storage/gate/principal/SID/Agent/issued parent/真实 CancelScope；foreground 断开只取消自己，background accepted 以实际 durable RunStart 为准；不同 Agent 的合法 child 保留父许可。

rust/crates/lingxi-service/src/runs.rs, rust/crates/lingxi-service/src/background.rs, rust/crates/lingxi-service/src/subagents.rs, rust/crates/lingxi-service/src/cancel.rs, rust/crates/lingxi-service/src/task_supervisor.rs, rust/crates/lingxi-service/src/session_supervisor.rs

## 物理文件退出

native read/write/edit 及 ResourceIO stat/read/list/search/write/write_expected_version/move/trash/materialize 的实际 blocking 闭包保留强许可；错误 actor/SID/gate/CancelScope 不降为手工 HTTP 旁路；原无 issued 手工文件编辑保持原行为。ResourceIO FIFO 使用新增可信宿主消费者，不声称旧 native 本有这个消费者。

rust/crates/lingxi-service/src/filetools.rs, rust/crates/lingxi-service/src/resourceio.rs, rust/crates/lingxi-service/src/sessionfiles.rs

## 进程与外围

沿原 R03/R04/R05 合同保留 process supervisor 同步发 kill/有界实际退出与 unknown 结果、worker callback accounting、MCP 连接收回。persistent terminal 原 Running 跨 tool call 生命周期不是每个终端永久持本次 Run 许可。resources blocking 为只读正文、management 为密码散列/登录、queue 为应用退出 join，均无本次会话分支写入。跨平台和外部不可确认作用仍 OPEN。

rust/crates/lingxi-service/src/procsupervisor.rs, rust/crates/lingxi-service/src/workerrpc.rs, rust/crates/lingxi-service/src/workermodel.rs, rust/crates/lingxi-service/src/mcpbridge.rs, rust/crates/lingxi-service/src/resources.rs, rust/crates/lingxi-service/src/management.rs, rust/crates/lingxi-adapters/src/storage/queue.rs

实际旧 Storage 装饰器全部转发同一 gate 和准入事实；纯 mock 的 None 仅保原假夹具。新增代码注释为简体中文，原英文未机械翻译。

共享 37 文件使其他候选、旧报告和旧运行证据不能继承到组合候选；后续 integration 和独立 Reviewer 必须重新运行。F01/F02/F04/F05/F06/F08/F15 及未由当前 Task 逐条亲验的原阶段义务保持原 owner/期限 OPEN。

最终绑定：当前focused134/0；whole1889/3 FAIL；正式R05/R04/R03全部真实FAIL，30注册命令各有真实exit，candidate均stable、runner均PASS。完整原Task/Stage继续OPEN，独立审查未运行。详见formal-chain-settlement.json、source-aggregate.json、scope-current.json。
