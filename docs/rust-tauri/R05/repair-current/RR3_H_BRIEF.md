# RR3 H / 新R02必需缺口F47与F48实施简报

你是全新rr3_h_impl_01，空历史，未参加RR3任何实施/审查/定位。全文读RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、最新矩阵/进度/HANDOFF、R02-TRIAGE-01/REPORT.md及全部根因原证/registration-proposal（真实已完成定位后才开始实现）。本轮仅R05，不R06。原规格16A/100+3C/130叶/I01-I11/N01-N16/原放行公式不缩，保留F31/F34/F41/F43/F44和新A/B/C/F46正确修复。F47/F48是原观察RR3-R02-OBS-A01/A13的新稳定别名，不宣称数据损坏，不重做旧28项。任何历史FAIL包括已隐藏请求编号证据均保留。

F47：现场a01_smoke较新DATA_EPOCH的guard实际拒启、原数据/备用根未改；脚本低日志级别把INFO选根诊断隐藏，却以缺诊断误判切根失败。独立定位已建议本R05必需修检查器的有效日志前置及全部现有断言。唯一所有权scripts/rust-tauri/r02_t01_service_smoke.sh、必要新的scripts/rust-tauri最小永久回归（先登记文件/根因/所有者）。核原R00只读叶及源码，不改DATA_EPOCH/迁移/实际存储保护，不删/降拒启、完整原文件/副根保留/配置来源三方向断言，不单改PASS字样或故意拿同名替身binary。真实旧脚本目标红→修后原正式shell完整绿（所有case及正确版本、根源三方向）；日志级别边界以实际配置为准，坏guard/错root拒绝反例只隔离copy，还原绿。

F48：真实auth错误体req-32hex与诊断日志的request_id应可关联，现长token脱敏吞掉整个字段。原R02关联契约及R05诊断义务必需，不是ALF。唯一所有权rust/crates/lingxi-service/src/redaction.rs及内联永久回归。必要邻接预登记rust/crates/lingxi-service/src/lib.rs与logging.rs，仅可信request_id/auth诊断字段调用与格式化路径，原F46open→prune/轮转/原上限3不变；若需要其他具体邻接先写REPORT登记并向总控给原因，不越白名单/产品scope。不靠放宽秘密保护或全局requestId前缀/长token免检取绿，不把任何调用者可伪造值当可信，不改模型/媒体/权限/usage等无关逻辑；普通最小方案自行决定并以原契约证明。保留所有原秘密形状/6preset/minted ticket等边界。永久正反控真实服务错误响应请求编号能匹配本轮真实authmarker；合成秘密含看似请求编号/拼接/URL/worker仍不得泄露，未知标识不造真实值，不关闭脱敏。旧目标红→修后正式r02_t07_redaction_scan.sh全绿包括correlation（不只SCAN），隔离仅破坏新防线红→精确还原绿。

构建与目标隔离：G-REVIEW-01正在negcopy.hTXzzN/共享NEG_TARGET跑完整默认16，不写/构建该COPY/NEG_TARGET、不动其evidence。使用根rustup绝对Cargo1.98.1 --locked；磁盘有限不能复制大target/.git或删用户/历史。可合理复用本主树buildcache但记录实际源码与binaryhash，D之前9f对象若重链接只是历史不得继续称当前。生产修改会使部分现有证据受影响，列清实际变更输入及C/F46/G影响，root后续安排另一新验收及新最终候选门禁。每个命令argv/cwd/UTC/exit/actualcounts/filtered/ignored/摘要输入/toolchain/locks/二进制与替身边界真实保存，只新RR3/H-01。

先按本brief与已完成triage登记两项最小根因改动，再实现自检；两个缺口同包完整闭合。不要读PASS字符串代替原证，不吞异常/空过滤/替生产入口。新注释中文。不写E现行docs/总控台账/authoritypins/任务书/R00叶/Git/系统、不派代理/外发消息。REPORT列所有修改、原红与修后绿/隔离目标红恢复绿、原保护复验及限制；你仅SELF_CHECKED，完成停止写交全新H验收者，不自签独立PASS。

追加已完成独立定位的精确条件：三来源cli/env/config-file完整A01预期17/0，以外层warn/info各真实亲跑；不能用本次定位长用户路径的无效精确pathgrep假绿。F48正式RequestId仍随机36字符/赋值47字符，不缩短成确定值取绿；真实AUTH/request handled/TRANSPORT与WS必要关联均核，保持用户路径脱敏范围。H自检完整资源160/54binary+61owner/15活worker/原阈值和最后清理需重跑（服务输入/二进制变了），还原日志旧防线/FD假零负控复用须逐项证明输入相等，不给新binary套旧hash。最终全新H验收及全新FINAL实际C资源覆盖，由root排队，不能以根因报告替代。
