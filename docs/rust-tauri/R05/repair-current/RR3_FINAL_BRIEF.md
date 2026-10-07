# RR3 全新最终阶段审查简报

只有总控派发后执行。你必须是从未参与RR3实施、修复、包级验收或前一最终判断的全新空历史智能体。全文读RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、最新RR3矩阵/进度/HANDOFF、各包全部新独立报告及关键原始证据，原§5.3/5.4/6.1/6.2完整继承。只R05收口，不R06，不修生产对象，不自签自己的实现。

## 派发前置与真实冻结

总控派发时核A最新独立PASS、B独立PASS、C/F46联合独立PASS、E当前文档独立PASS、H新增F47/F48及受影响C/F46资源新独立PASS、I/F49独立PASS、J/F50默认依赖准备新独立PASS、G-REVIEW-02或后续新轮当前默认N01–N16全独立PASS；D历史构建对象已有全新D审查，最终重链接对象须本轮实际核对，必要外部系统解除尚未执行时如实BLOCKED，不能给其新增延期豁免。环境已证BLOCKED仍亲跑全部可执行正式检查，列全实际失败，不只首workspace错误。

所有主树所有者及总控在正式运行期间静默写入。最终审查的CLI dispatch/过程日志/命令元数据/临时报告须先放仓库外真实/private/tmp新目录，不往主树其他证据目录写。正式入口只写新FINAL-01/verify-R05，不预写该evidence root。控制性外置输出结束再归档RR3/FINAL-01，保存外置→归档路径/hash一致；不能排除整个artifacts/所有untracked或重拍快照接受漂移。

读取真实Git分支/HEAD/远端及未提交差异，新增提交审增量；基线b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b只作基线，不假称所有工作树改动已在该SHA。锁定根rust-toolchain.toml；rustup绝对cargo/rustc1.98.1，Node24.16.0/npm11.13.0、macOS arm64，锁/schema/config/fixture/binary hash完整。未改变生产输入且有效同候选证据可按逐项hash复用，受影响必须重验，默认16已G新亲跑不拿旧RR2替代。

## 原§5.3全部亲跑

顺序保存每命令argv/cwd/UTC/exit/测试数ignored/filtered与原始stdout/stderr，不吞退出，不空过滤、不ignore，不替换被测生产入口，不故意延迟r00或绕过LAN。

```sh
/Users/study_superior/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check
/Users/study_superior/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --workspace --locked
/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR3/FINAL-01/verify-R05
```

正式R05必须实际执行当前候选R04前序闭包并解析内嵌R03/R02/RR1，不能拿旧PASS。原§5.3允许同一有效编排复用避免无意义重复；若完整正式R05实际执行/验证了R04及全部下层，记录它就是本轮亲跑前序，不能伪造单独R04命令；需要单独入口时新ENTRY目录，不能同目录重跑。重跑最终入口换FINAL-02并换全新阶段审查者，保留本轮失败。

递归每份真实verify-stage-result.json：commands与overall逐项、candidateSourceBinding.stable、每checkpoint、runnerSourceBinding.status、source/copy/digest/依赖引用/所有文件hash及缺失。列完整失败命令/叶及各自原因；stable=false独立FAIL不能被LAN遮盖。R02 raw npm红及合法directed/E5范围原样报告，不能写全绿。r00每次确切binary路径/SHA/CDHash/监听/入站行为与D准备对象核对，任何重链接需按真实身份更新操作准备，不能旧同名Allow视作授权或PASS。

## 放行公式、审查报告与提交边界

核原16A、100+3C、130适用叶、I01–I11及所有F-ID/新增F45–F50真实份额与独立证据闭合，保留原许可LIVE/平台延期。只有原§6.1全成立、无必需OPEN/SELF_CHECKED/BLOCKED且新终审PASS才accepted/R06_READY=true；系统必需阻断时overall/independent_review按真实FAIL、offline_gate按真实FAIL或BLOCKED、NOT_ACCEPTED/false，不实施R06。

STAGE_REVIEW.md与结构化终审总结包括各层结果/全部失败/候选时间/绑定/未执行边界/下一准确命令。冻结生产输入清单应涵盖实际被测源码、依赖lock、配置/协议schema/fixtures、authority maps/pins和所有运行脚本（新增辅助不能漏）；同时保存完整candidate binder观察。后续仅文档回执变更要以逐项清单证明被测生产输入相等，不把后续提交SHA冒称tested SHA，不借此隐藏运行输入变化。纯报告输出与生产读入文件边界需要有源码/命令依据，不能随意把docs全排除。

所有最终产物归档artifacts/rust-tauri/R05/RR3/FINAL-01，历史只读。你不执行Git写入/系统权限变更/发布/外部消息、不再派代理；总控在报告后精确提交推送及文档补录，另新文档审查收口。不改审计门禁/封印坐标/白名单以使测试绿。磁盘有限，不复制大target树/删用户内容；真实环境限制按UNKNOWN/BLOCKED保存，能执行的检查继续完成。

完整失败列表：原workspace命令仍须原样亲跑保留；若其默认遇失败提前停止后续test binaries，须用新独立证据命令 `cargo test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast`（同绝对cargo）补齐未执行范围并列全失败，而非只报首r00。这不是替换原§5.3命令或忽略失败；不要无新信息反复跑同失败。所有额外耗时命令仍记录实际exit/超时/被杀，不凭推断宣布后续通过。
## FINAL 派发时附加事实（总控，G03 完成后归档进 RR3_FINAL_BRIEF.md 尾部）

- 前置状态：A/B/C-F46/H/I/J 全部包级独立 PASS；E03 文档轮已经 E-REVIEW-04 PASS 无 mustFix（artifacts/rust-tauri/R05/RR3/E-REVIEW-04/REVIEW.md）；G03 结果=包级 PASS 无 mustFix（G-REVIEW-03/REVIEW.md：default16-03 十六项 fail-closed 点名+controls 绿+真实 exit0；full R02/E5 20/20 overall PASS；N16 恢复独立复算全等）。G-REVIEW-01 默认 FAIL、G-REVIEW-02 BLOCKED_BY_STORAGE 为历史保留，不冲销。
- 空间：总控已 cargo clean 主树 rust/target（280.7GiB，回执 TASK0/cargo-clean-receipt.txt，Data 可用约 583Gi）。主树 target 为冷缓存——你的 §5.3 构建从冷开始，耗时属正常，如实记录；不得因耗时长而跳过或复用旧二进制。
- D 对象身份：主树重编译后 r00 测试二进制将与历史 9f7489029c91d1c… 不同——按 brief 既定流程以本轮实际对象的路径/SHA/CDHash 重新核对，历史对象不作为当前授权或 PASS；LAN 入站仍预期被 ALF 阻断（0 字节/超时），如实 BLOCKED 列出，不全局关防火墙、不跳过断言。
- 环境修正：先 `echo $HOME`，若非 /Users/study_superior 则 `export HOME=/Users/study_superior` 并记录；cargo 绝对路径 /Users/study_superior/.cargo/bin/cargo。
- 唯一证据根 artifacts/rust-tauri/R05/RR3/FINAL-01/verify-R05（不得预先存在；重跑换 FINAL-02）。
- 期间 root 完全静默（不写仓内任何文件），直至你全部命令与子进程停止。

## FINAL-02 派发附加事实（总控，F51 关闭后）
- FINAL-01 为历史 FAIL 保留（命令1–5全绿含workspace 1486/r00 LAN通过；cmd6绑定器拒56夹具）。F51已修复并经F51-REVIEW-01独立PASS：56嵌套.git夹具已迁至仓库外LingxiAgent-RR3-localonly-fixtures/（RELOCATION-RECEIPT.json+41原位标记），主树枚举目录条目=0、tracked零变化、xtask candidate回归9/0绿。
- 你的证据根=artifacts/rust-tauri/R05/RR3/FINAL-02/verify-R05（不得预先存在）；报告与命令记录放RR3/FINAL-02/下其余文件。§5.3六条命令全部重跑（不因FINAL-01已绿而复用其结果——同候选输入相等复用仅限你在本轮运行内自行记录）。
- r00预期：FINAL-01轮对象43d95970…（CDHash 4ab00dfe…）ALF已放行且LAN通过；本轮若复用暖target同对象应再现通过，若重链接新对象则如实记录新身份与LAN行为，不做系统修改。
- 其余约束沿本brief原文与既有派发附加事实（HOME修正/冷暖缓存如实记录/全程df/外置→归档）。完成后停写交总控。

## FINAL-03 派发附加事实（总控，F52 关闭后）
- FINAL-01（56嵌套.git目录）与 FINAL-02（1 symlink）均为历史 FAIL 保留；F51/F52 两轮证据卫生修复均经全新独立审 PASS（F51-REVIEW-01、F52-REVIEW-01）。绑定面经实施者三跑+审查者独立分类器终量复检（69,140条）=100%普通文件、零目录/零symlink/零其他。
- 你的证据根=artifacts/rust-tauri/R05/RR3/FINAL-03/verify-R05（不得预先存在）。§5.3六条命令全部重跑；verify-stage R05 预期首次穿过候选绑定并执行全部层级（R05 suites+R04/R03/R02/RR1闭包），耗时长属正常，逐层落盘递归核验（commands/overall、stable、全部checkpoint、runnerSourceBinding、摘要/依赖引用）。
- r00：FINAL-01/02 同对象43d95970…（CDHash 4ab00dfe…）连续两轮 ALF 放行且 LAN 通过；本轮若同对象应再现；若重链接如实记录新身份与行为，零系统修改。
- 若本轮全部通过且§6.1成立：六元组如实翻 accepted/R06_READY=true 前仍须核对全部包独立PASS链与 E04 回填安排（终审结论先行、文档回填随后由总控另派，不阻塞你的判定）。完成后停写。

## FINAL-04 派发附加事实（总控，F53/F54 关闭后）
- FINAL-01（56嵌套.git目录）/FINAL-02（1 symlink）/FINAL-03（flake+46叶分类）均为历史FAIL保留；F51/F52/F53/F54 四轮修复均经全新独立审 PASS（F51-REVIEW-01、F52-REVIEW-01、L-REVIEW-01、M-REVIEW-01；M含审查者独立standalone verify-stage R04 overall=PASS复现）。
- 你的证据根=artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05（不得预先存在）。§5.3六条命令全部重跑；命令6预期全层级通过——预计总时长2.5-3小时，从启动即用 python start_new_session 完全脱离宿主会话（FINAL-03尝试1被宿主SIGKILL的教训），不要中途回attch。
- 运行期间总控与一切其他代理完全静默（M attempt-1并发写证据致digest漂移的教训）：你是唯一仓内写者，除FINAL-04/与rust/target构建副产物外不写任何路径。
- r00：预期复用43d95970…对象（L/M均未改lingxi-service生产源码）ALF连续三轮放行；若重链接如实记录新身份与行为。
- 若本轮全部通过且§6.1成立：六元组如实翻（offline_gate=PASS、independent_review=PASS、stage_readiness=ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS或按真实许可口径、R06_READY=true），列明合法LIVE/平台延期边界；E04文档回填与Git提交由总控随后执行，不阻塞你的判定。完成后停写。
