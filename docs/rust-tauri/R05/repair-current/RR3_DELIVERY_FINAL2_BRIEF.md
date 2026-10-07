# RR3 DELIVERY-FINAL-02：放行后最终精确交付清点（仅总控明确派发）

你为全新空历史只读交付准备者。全文读 RR1/RR2 MASTER、RR3_BRIEF、最新 RR3 交接/矩阵/进度、RR3_DELIVERY_FINAL_BRIEF.md（DELIVERY-FINAL-01 的任务书——其中"磁盘仅约265MB"等低磁盘约束已被解除，其余分类纪律全部继承）、DELIVERY-PREP-01/REPORT、DELIVERY-PREP-02/REPORT 与 REFRESH_RULES、DELIVERY-SPACE-01/REPORT 与 normalization-boundary.json、DOC-INPUT-BOUNDARY-01/REPORT、FINAL-04/STAGE_REVIEW.md、E-04/E-REVIEW-05 报告。当前所有生产/文档/测试作者已停写；root 授权的 commit/push 由 root 执行，你不写 Git/主源码/现行 docs/系统，不删除任何原件，不运行产品测试/构建或历史 driver，不派代理。唯一新输出 `artifacts/rust-tauri/R05/RR3/DELIVERY-FINAL-02/`。中断的 DELIVERY-FINAL-01 部分采集（inventory.jsonl 等）保留为历史，可只读参考但不当代数。

## 本轮事实基准

- RR3/FINAL-04 全新独立终审 PASS：六元组 accepted（offline_gate=PASS、independent_review=PASS、stage_readiness=ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS、R06_READY=true）；E04 回填+E-REVIEW-05 独立 PASS。Git 至今零暂存/零提交/零推送。
- 磁盘约 513Gi 可用：输出规模不再是硬约束，但仍不重复复制大库存（引用+增量）。
- F51/F52 外置夹具：56+1 个嵌套 .git/symlink 夹具目录已迁至仓库外 `/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures/`（回执 F51-01/RELOCATION-RECEIPT.json、F52-01/RELOCATION-RECEIPT-F52.json、41+1 原位 RELOCATED 标记）——全部 localOnly、remoteOriginalAvailable=false，按回执登记，原 manifest 不删改。

## 任务

1. 按 `git ls-files --others --exclude-standard -z`、`git diff --name-only -z`、`git diff --cached --name-only -z` 重新权威枚举（应无 staged）。本轮范围=RR3 全部改动（25 M tracked + RR3 未跟踪证据/文档）+白名单授权生产文件；HEAD 应为 b3ac0e6a。PRECISION：DELIVERY-PREP-02 的 19734/13828 集不是最终清单——PREP02 之后新增的 G-REVIEW-03、FINAL-01..04、E-04、E-REVIEW-05、F51/F52、L/M、M-REVIEW-01、E-REVIEW-04、G03 附加等全部新材料与 E04 的 12 份文档变化、M 的三文件、L 的测试文件必须实际分类；旧已判类别仅在身份仍成立且使用角色不变时继承。
2. 分类纪律沿 DELIVERY_FINAL_BRIEF 原文：保留真报告/历史 FAIL/命令退出/原日志/原始测量序列/独立 driver 与故障变异恢复说明；故障工作 copy 即使恢复仍 localOnly；运行时真实临时票据/token/home/数据库仅本地；合成测试常量不简单按 secret 词删；混敏感值日志保留原件并单列必要无值摘要。对每个排除且被正式证据引用的原件登记 SHA/bytes/driver/source、localOnly=true、remoteOriginalAvailable=false。>134MB 单文件（如大 rlib）仍 localOnly。识别用户要求的交付义务有无真实未满足。
3. 输出：全路径唯一分类（include/local/unknown 互斥覆盖）、精确 NUL 分隔列表（root 后续逐文件暂存用）+人类可读 txt、每文件 SHA/size/mode 及类别依据、引用边界索引、正常与不明条目分别记录。JSON 严格无重复键；自身新证据用独立收据描述避免循环自hash。仓内 source/files 前后稳定核验；记录 UTC/命令 exit/真实 HEAD/index/staged 但不写 Git。
4. 163 条 CRLF 旧原证：消费 DELIVERY-SPACE-01/normalization-boundary.json 的逐路径对照，精确标记需 --no-filters 写入+update-index 保留原始 blob 的路径与 raw/filtered 对照（不改 .gitattributes/原件/全局配置）。
5. 只读估算 Git 写入所需空间（blob 头+zlib 长度+index/tree+push pack 余量，保守），当前余量约 513Gi 应充分——给出数据而非盲承诺。
6. 完成或列全 UNKNOWN 后停写，交 root 派 DELIVERY-REVIEW-01 全新独立审查。
