# R3 未跟踪证据日志脱敏审计（recheck 轮）

- 审计时间：2026-09-22（本轮）；审计者：本轮修复执行 agent。
- 范围：`EVIDENCE_SHA256.txt` 引用但 git 未跟踪的 123 个 `.log` 文件 + 本轮
  `recheck-r1-r2/` 新增日志（共 136 个文件逐个读取扫描）。
- 规则：私钥块、OpenAI/Groq/Google/GitHub/Slack/AWS 形态密钥、credential 赋值、
  Bearer 令牌、邮箱、40+ 字符高熵串（排除 git sha 与超长 base64 载荷）、真实用户
  数据目录标记（`~/.hanako`、真实 Application Support Data）。

结果：

1. 密钥/令牌/私钥/credential 赋值：**0 命中**。
2. 邮箱：7 个文件命中 `i@izs.me`——npm 废弃告警样板文本（glob@7 等第三方包的
   公开维护者地址），非用户数据，不需要脱敏。
3. 高熵串：全部为开发机绝对路径（`/Users/study_superior/...`、`/var/folders/...`
   临时目录）。与同交付树已跟踪的 `commands.jsonl`（含相同 cwd）先例一致，
   属于验证环境坐标，不是用户业务数据。
4. "Application Support" 命中（6 个文件）：全部指向本轮/前轮**隔离测试目录**
   （`Lingxi-f05-packaged-run-rIw7Tw`、`Lingxi-f05-soak-ghxm6w-data` 等随机后缀），
   是隔离验证的 lsof 快照；未发现真实用户会话/消息/凭证内容。
   P00-A03 首次 GUI 隔离缺口的事故事实保持原报告记录，不以入库证据弱化。

结论：123 个未跟踪日志与本轮新日志均无密钥或真实用户数据，可加入版本管理；
不做内容改写（哈希须与既有清单一致）。
