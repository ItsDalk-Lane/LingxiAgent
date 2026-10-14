# EX01最终保存竞态修复增量

本增量只修复独立审阅确认的最终保存竞态。与此前交付的 /tmp/lingxi-ex01.patch 串联应用；不包含W11、memory/persona等同批功能、不改门禁/登记/台账，不提交或推送，也不表示R06已验收。

## 根因与修复

原HTTP复检之后还有实际异步数据库读取和写队列等待。即使辅助摘要usage已落盘、manual档案尚不存在，资料/工具/配置变化仍能越过旧复检。原取消phase与分支CAS已有最终保护，但资料同代保护没有进入真正COMMIT。

新增仅供manual档案使用的guarded事务：拿到SQLite写锁、消息/分支头暂存后，在实际COMMIT回调内重新编译并比较原materialSha256/materialRevision/render，读取当前真实compaction配置；工具目录必须仍为原catalogGeneration。任何不一致返回Conflict并ROLLBACK，消息、分支头和文件引用一起撤回。普通with_write_txn函数保持原逻辑。

真实资料发布栅栏增加短暂最终保存保留位。只在无活动资料写者时获取；若已有写者立即拒绝，避免持有DB写锁等待资料写者的锁序循环。原嵌套/并行写者计数保留；新资料发布须等最终保存保留位释放。同步采集与SQL COMMIT期间同持资料栅栏、工具目录锁；取消CAS在栅栏内紧接COMMIT裁决，取消成功与保存决策不能同时成立。锁中没有异步DB等待，也不再次获取工具目录锁。

服务生产资料写入继续走既有ContextMaterialMutation（助手配置的实际PUT落盘也在其中）。其他进程直接文件编辑不会遵守本服务发布锁，沿既有双实际采集语义，以最终保存回调中的稳定采集结果为线性化点；本增量保证等待期间已发生的外部编辑会拒绝，未声称能给任意外部编辑器提供跨文件/数据库物理原子提交。

## 真正时序验证

新增4个HTTP端到端场景。仅远端模型替身，HTTP认证、ModelGateway、AuxiliaryExecutor、driver、ContextCompiler、ToolRegistry、SQLite和ModelHistory均为生产链。

每个场景必须先观察该会话真实auxiliary.summarize付费记录，再由独立SQLite连接取得BEGIN IMMEDIATE，锁内确认manual档案COUNT=0；随后实际生产队列查询超时证明写者正在等待。仅在此已到达窗口注入：外部助手改名、实际工具目录禁用、真实PUT reserveTokens修改、生产取消域获准。允许至多12个新会话尝试捕获窗口，但没有捕获则必FAIL，不能以重跑或无观察PASS替代证据。最终有效修前/修后运行均在第0个会话捕获。

修前负控用先前交付生产文件的精确副本、同一新增测试、强制重编：材料/工具/设置3腿均返回200并在原409断言FAIL，取消腿409 PASS。日志 /tmp/ex01-late-negative-control.log，exit101，1PASS/3FAIL。修后4腿全部409，原消息+head+revision+Run计数逐字相同，manual档案=0，session_operations操作收据及key_events数量不增加，无compaction_succeeded，真实usage仍在且cause_ref为accepted的真实action，不生成假Run/attempt。

另外2个资料栅栏并发检查证明嵌套写者必须全退出、跨线程实际新发布无法穿过保存保留区；1个工具目录并发检查证明实际set_availability不能穿过目录锁，旧世代闭包必拒绝。这3个是锁语义检查，端到端提交时序证据仍来自上述4个真实SQLite窗口。

## 运行结果

- r06_ex01_manual_compaction 10/10（原6+新增4）；r06_ar_f01_history 8/8；r06_session_history_content 12/12；r06_t02_closed_loop 2/2；r06_t02_compaction 26/26；r06_t03_session_tree 14/14：/tmp/ex01-late-regression.log。
- adapters session_tree_r06t03 18/18、context_material_commit_r06ex01 2/2：/tmp/ex01-late-adapters.log。
- kernel final_commit_generation_lock_blocks_actual_catalog_mutation 1/1：/tmp/ex01-late-kernel-fence.log。
- 合计93场景（原86+新增7）PASS；Clippy service/adapters/kernel --lib -D warnings PASS（/tmp/ex01-late-clippy.log），cargo fmt --all --check与git diff --check PASS。Clippy首次指出回调签名过于复杂，已用命名类型消除，未放宽检查。

cargo均为指定绝对路径 /Users/study_superior/.cargo/bin/cargo --locked；工作树 /tmp/lingxi-w11-codex-20261014；独占target /tmp/lingxi-ex01-target；LINGXI_HOME /tmp/lingxi-ex01-env/home、TMPDIR /tmp/lingxi-ex01-env/tmp、LINGXI_PRODUCT_DIR指向该独立树lib。主树只读，原始运行目录不提交。

未验证真实供应商、其他平台、断电/磁盘满和全R06门禁。slash/WS完整事件/UI入口等前次未完成面仍维持原结论；本报告不能代替独立复核或总验收。
