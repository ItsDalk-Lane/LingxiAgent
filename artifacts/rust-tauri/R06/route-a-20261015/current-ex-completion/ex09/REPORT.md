# EX09 引用位置适配独立复核

结论：PASS_SCOPED_EX09_UTF16_CITATION_ADAPTATION。当前Host可验证范围内，公开引用偏移已正确适配现役JavaScript字符切分，旧内部事实/原始内容/收费向量未被重写。没有改Main、门禁、登记或原断言，没有提交推送，也不给阶段总体结论。

## 当前Main真实复跑

主控提供的新26案实际父二进制 `r06_original_knowledge_proofs-a7bf095ff7da297b` 精确运行 original_citation_non_bmp_offsets_match_current_viewer_and_survive_restart，严格确认running1 / 1passed / 0failed，非只看exit0；运行前后bin SHA稳定。current-main-new-parent.log / current-main-new-parent-receipt.json完整保留。真实HTTP引文JSON另存 current-main-new-http-body.json，并直接交给下述当前组件JS表达式，结果 current-main-new-dialog-slice.log：前缀😀前文𝄞、标记证据甲乙、后缀🦊尾文准确，拼接等于原文。

首次主控先前给的旧25案路径精确filter实际running0 /25filtered，本审查没有算PASS，诊断current-main-parent.log保留。随后使用新路径才得到上述实际1PASS。主控自己26全套PASS不被当作本审查独立26全跑。

## 新独立边界

自有私树仅复制Main四处引文/read/grep精确生产hunks作为实验前置，额外运行真实服务与模型供应商HTTP替身；其它Main生命周期/EX06增量不反向覆盖。相关函数/字段逐hunk SHA完全相等，见source-proof.json。原新增父在该实验候选也真实1PASS。

新增独立场景原文：😀前缀𝄞证据🦊甲乙🧭后缀。选中目标𝄞证据🦊甲乙，前缀、目标和后缀都实际含非BMP。真实HTTP读取引文后，用TypeScript AST从当前KnowledgeCitationDialog.tsx提取其原始三条表达式，在真实Node JavaScript上执行：slice(0,startOffset)、canonicalText、slice(endOffset)。再核对真实slice(start,end)等于canonicalText，三段拼接逐字等于原文。没有自己重写一套字符索引函数替代消费者。旧内部scalar偏移套入同一表达式作为负控，明确被拒，legacyControlRejected=true。探针current-dialog-slice.cjs、原始HTTP事实independent-http-body.json、结果current-dialog-slice.log、实际Rust探针independent-probe.rs与日志independent-probe.log全部保留。

此场景用真实嵌入HTTP供应商替身及正式费用链先生成非空向量1条、真实费用1条，再读引用/读范围/grep并冷重启；旧引用整行、原始快照字节、非空vector BLOB行及整行费用记录在前后与重启后均严格相等。没有用空向量/零账单冒充保全证明。费用只来自实际摄入，引用读取及展示不新增费用。

同一场景实际调用生产read_citation_page与grep_scope（真实冻结scope，非mock核心）：原startOffset/endOffset仍为Unicode scalar，offsetUnit明确，等价utf16StartOffset/utf16EndOffset与HTTP公开引用一致。此两项是直接生产服务方法验证，不能冒称额外HTTP入口/完整模型Run已跑。

## 实现核对与边界

citation_json先验证旧scalar范围、canonicalText及其SHA确实对应保留原文，再仅在公开JSON把边界换成UTF16；旧citation数据库值不变。read/grep原scalar字段保留并显式单位，另加等价UTF16。当前Dialog实际使用公开偏移对原block.text切prefix/suffix，mark正文取同一canonicalText；本次动态检查同时覆盖边界截断和重复/漏字。

两次实际独立case均PASS，无跳过。自有扩展probe复用原文件前置辅助时有一个未用常量dead_code编译警告；没有把该probe声称严格Clippy通过，Root业务源未因此改动。

本次未运行完整React挂载/请求生命周期、Electron/Tauri窗口、其他平台、完整26项独立重跑或正式阶段门禁。真实JS表达式验证不能冒称完整用户界面已经验收。完整机器范围与候选源码见confirmation.json及source-proof.json。
