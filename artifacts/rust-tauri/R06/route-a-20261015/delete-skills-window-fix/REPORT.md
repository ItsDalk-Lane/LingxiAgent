# 599窗口唯一失败的夹具修复独立只读复核

结论：最终 model-only 配套静态 PASS，未弱化原断言。本次产品/测试源码唯一 diff 是 r06_agent_delete_skills_live.rs 的 boot：声明合成 chat route、原 CredentialService 与 gateway，并交入 bootstrap。没有 workspace、没有生产/UI/门禁判定改动。读取期间Root另刷新对应生成登记锚点，故后续git diff还含inputs/graph/anchor报告；原叶binding JSON实核仅sourceSHA和行坐标变化，未改原断言文字/命令。

原全量失败 r06_test_6aa5156db27be9a5331b 发生在 last 子场景准备 DELETE survivor（UI71的mutation），不是最后 victim 的实际拒绝检查；primary survivor 被视为当前助手，正确生产会先创建替代 victim 会话，旧default deps没有模型。只补可派发合成模型是合法准备，不执行供应商生成。读盘现场、原JSON与完整只读根因另存 /private/tmp/r06-delete-last-fixture-review/，未覆盖原失败。

独立逐字核对：从第一个 #[tokio::test 起全部原测试正文，与HEAD逐字节相同；真实UI源与HEAD逐字节相同。原401、last HTTP400、技能/配置/bundle/tombstone保全、候选变化/清理失败、双连接拒绝及结果数量断言均未动。boot无cwd，不引入新配置历史写入。第一修补加入workspace曾碰到旧config字节断言的失败，Root已保留r1.log并删掉多余workspace；没有通过修改断言掩盖它。

报告与确认：confirmation.json保存HEAD/base/candidate/diff/UI SHA，fixture-only-reviewed.diff保存所审净hunk。本审阅者没有运行任何构建、测试、frontend或HTTP；Root r2日志读取时仍在执行，不提前声称2PASS。修后须由Root真实原两父和随后同新候选干净全集/正式R06证据证明；旧598PASS不自动升级为新候选PASS。该配套静态结论不等于阶段验收。
