# 候选快照内存共享：只读设计复核

结论：在所述精确前提下静态方案 PASS；这是完整重新校验后的不可变结果去重，不是减少校验或缓存文件结果。本轮未读到最终已落地实现，不虚构最终源码SHA；未改Main、未构建或测试。

当前 Entry 已有逐值 Eq，覆盖 path_hex/display/kind/sha256/mode。Scope.snapshot 每次完整列举/secure hash/检查模式与身份后，只有 digest 和整份有序 Entry Vec 均相等才将新结果 entries 换成 baseline 的 Arc clone，内容、顺序、to_json、differences 和计数都不变。当前全部消费者仅len/iter/值比较，没有对外修改Entry的调用。Snapshot Clone从复制改为共享在这些只读路径可接受。

必须保留不同快照和Err的独立记录；不能用digest alone或ptr_eq代替双重值比较，不能拿新恢复的baseline覆盖旧changed checkpoint；before/after、每命令checkpoint、runner identity、错误拒绝、完整JSON全部保留。不要引入可变共享。相同结果的新Vec在比较/替换后释放，保留Arc引用即可；真实持续变化时仍要保留不同快照，不为省内存抹去失败证据。

新增单测除真实修改/恢复外，建议显式伪造相同digest但Entry或mode不同以证明不能共享，以及digest不同但Entry相同不共享。真实文件变更后旧changed checkpoint即使末态恢复仍拒绝；Err仍拒绝；共享前后完整JSON和changedPath不变。原candidate全组、parallel parity与安全边界、runner嵌套fd/checkpoint组和strict/fmt/重编仍应执行。Root负责动态验证，本报告不代签测试通过。

没有发现方案本身的具体阻碍。它仍每次重新分配一份临时完整快照并比较全部Entry，故只减少持久堆积，不减少hash工作；实际内存/耗时改善需测量。正式进程已停止的旧窗口不因本方案而升级通过，采用仍须新候选与新鲜正式证据。
