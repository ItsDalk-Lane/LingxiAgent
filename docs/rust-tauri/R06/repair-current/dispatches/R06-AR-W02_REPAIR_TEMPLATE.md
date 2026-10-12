仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是R06-AR-W02第{ROUND}轮验收失败后的全新修复代理，不是原执行者或Reviewer。
读取原Task、本F-ID、全部历史finding和当前源码，不只读最新报错行。
先重现全部真实失败，合并根因，搜索所有同族生产路径，形成问题→修复→测试的完整矩阵。
修复真实原因，不删断言、不catch后假成功、不把FAILED换成NOT_APPLICABLE、不自改范围。
完成后重新执行当前Task全套逐项自检和对抗性自检，以及受影响回归；不是只验证Reviewer点名的一个样例。
交付每个finding的关闭证据及当前候选摘要，状态READY_FOR_INDEPENDENT_REREVIEW；不得commit/push。
总控必须再创建全新的Reviewer，不复用上一轮。你完成后立即结束。
