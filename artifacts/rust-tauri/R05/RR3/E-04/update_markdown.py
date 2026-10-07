#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""RR3 E-04 Markdown 回填：主报告/独立索引/阻断/负测/usage 说明的当前节翻转。

历史节原文保留，仅加"已取代"标记；WORKER_MODEL_BOUNDARY.md 与
R05_INTERFACE_EVOLUTION.md 不在本脚本触碰范围（字节保持）。
"""
import os

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
BANNER = (
    "> **RR3 E-04 生成截点（2026-10-08）：stage_readiness=ACCEPTED_OFFLINE_SCOPE_"
    "WITH_REGISTERED_LIVE_DEFERRALS / R06_READY=true / offline_gate=PASS / "
    "independent_review=PASS。** 依据 RR3/FINAL-04 全新独立终审亲跑：§5.3 六条命令"
    "全部真实 exit=0，verify-stage R05 三层（R05/R04/R03）overall=PASS、stable=true、"
    "checkpoint 全稳、runner 全 PASS、testedSha=b3ac0e6a+真实工作树，失败清单为空；"
    "F42–F54 全部独立 CLOSED；r00 两新对象 cf9bce2f…/d57ea731… LAN 6 次实测通过且 ALF "
    "放行（无证据需要用户操作）。LIVE=BLOCKED_NOT_AUTHORIZED（原许可最迟 R10）、"
    "Linux x86_64 继承未复验/Windows 未验证（R09/R10）原边界不变；raw npm 历史 candidate"
    "红保持登记不写全绿。Git 至今零暂存/零提交/零推送（FINAL-04 亲核），本 E04 不预写提交"
    "回执。本 E04 仅 SELF_CHECKED，待全新 E-REVIEW-05；现行范围见"
    "[R05_REPORT §13](R05_REPORT.md#rr3-current)，此前各轮原文（含 §12 E-03 截点）均保留"
    "为历史。"
)

REPORT_S13 = """
<a id="rr3-current"></a>
## 13. RR3 E-04 FINAL-04 放行状态与交接（2026-10-08）

**stage_readiness=ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS / R06_READY=true。** §1–§12、E-03 空间阻断截点与"FINAL 从未执行"均为历史；本节消费 [RR3/FINAL-04 全新独立终审](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/STAGE_REVIEW.md)（[结构化总结](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/STRUCTURED_SUMMARY.json)、[证据根 verify-R05/ 750 文件](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05/verify-stage-result.json)、[command-records 36 文件](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/command-records/)、[尝试1 中断现场](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05-ATTEMPT1-INTERRUPTED/)）的真实放行结果，不预造任何未来动作。

### 13.1 六元组（FINAL-04 正式结论）

```text
offline_gate:            PASS（§5.3 六条命令全部真实 exit=0，含命令 6 三层 gate overall=PASS；原始 stdout/stderr 落盘 command-records/）
independent_review:      PASS（FINAL-04 全新空历史审查者亲跑全部六条命令并递归核验三层 JSON；原 §6.1 八条全部成立）
live_verification:       BLOCKED_NOT_AUTHORIZED（RR-BLK-CREDENTIALS，原许可最迟 R10，未执行不伪造）
platform_verification:   macOS arm64=本轮全部真实执行；Linux x86_64=继承原登记未复验；Windows=未验证（R09/R10）
stage_readiness:         ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS（离线规定范围全部通过；剩余仅为原许可 LIVE 延期与合法继承的平台义务）
release_state:           NOT_IN_SCOPE
R06_READY:               true
```

### 13.2 候选、命令与三层结果

- 候选：HEAD `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`（origin 同）+ RR3 未提交真实工作树；开工==收尾 `git diff HEAD` SHA256 `7041cafb…` 与 `git ls-files -s` SHA256 `3016f7ae…` 前后逐字节相等；tracked 集 FINAL-01 起未变（29 M = FINAL-03 25 M + L/M 4 文件）。**零 Git 写**（reflog 顶条仍提交 b3ac0e6a、index 未重写、零暂存）。
- 窗口：命令 1 于 2026-10-07T19:56:20Z 起 → 命令 6 尝试 2 于 22:46:35Z 止（gate 4912.7s≈81m53s；两尝试合计 2h32m54s）。命令 6 尝试 1 被宿主终止（连脱离会话的 xtask 一并被后代进程树清理杀死，gate UNKNOWN 非产品失败，192 文件字节保留）；尝试 2 double-fork 孤儿化完整跑完 exit=0，为本轮签收依据。
- 命令 1–5：fmt（0 输出）/clippy（0 警告，L 测试改动触发真实部分重编 53.06s）/workspace（115 组 1486 passed/0 failed/0 ignored/0 measured/0 filtered，14m52s，含 r00 LAN、resources、closed_loop 全绿）/check-contracts（56 生成文件 drift-free + API_COMPAT_MATRIX 626 条）/check-boundaries（RESULT: OK）。

| 层 | overall | 命令 | checkpoint | 绑定 | runner | 场景 | 叶表 |
|---|---|---|---|---|---|---|---|
| R05 | PASS | 7/7 exit0 | 7/7 stable changed=0 | before==after（72,289 文件） | PASS | 18/18（A01–A16+SUP×2） | **130/130 PASS**（124 share+6 full）/0 fail/0 deferred |
| R04 | PASS | 8/8 exit0 | 8/8 stable | before==after（72,402） | PASS | 24/24 | 55 PASS（46 full+9 share）/0 FAIL/69 deferred |
| R03 | PASS | 15/15 exit0 | 15/15 stable | before==after（72,475） | PASS | 17/17 | 17 PASS/0 FAIL/31 deferred |

- testedSha 三层均为 `b3ac0e6a…`+真实工作树；`completeFailureList=[]`；33 项冻结生产输入与 FINAL-03 基线交叉核对一致（唯一差异 stage_maps/R04.json 为 M/F54 生成器重建，预期；开工快照被收尾复跑覆盖的过程失误以三重证明补救，见 STAGE_REVIEW §二）；开工绑定面 72,097 条=100% 普通文件（F51/F52 修复保持）。
- R02/RR1 口径（如实）：R03 层 r02_* 命令全 PASS；legacy 为 **directed（E0–E4.5 全绿、E5 BY SCOPE SKIP，原明确许可）**；full E5 属负测 N16 范围，由 [G-REVIEW-03 隔离副本](../../../artifacts/rust-tauri/R05/RR3/G-REVIEW-03/REVIEW.md)独立证明（N16 run-b 20/20 命令 overall=PASS + a16 E5 全量+seal-family 分类 GREEN，历史有效，本轮不重跑不新签）；**raw npm 历史 candidate 红（seal trio 3 文件/6 失败）保持登记 registered-not-formal-green，未写全绿**；RR1 repair_suites 与 r04_rr1_repair_suites 均 PASS。

### 13.3 r00 对象与 LAN（D 项）、环境项

- 任务书预期复用 FINAL-01/02/03 对象 43d95970… 未成立（如实记录）：命令 3 用重链接对象 `cf9bce2f…`；命令 6 启动后 20:14:37Z 再重链接为 `d57ea731…`/CDHash `364514be…`（gate 内 5 次 workspace 用此对象）。重链接归因 cargo 指纹判定，非源码变化（三层绑定 digest 前后相等佐证）。
- **LAN 本轮 6 次全部真实通过**（命令 3 + 尝试 1 两层 + 尝试 2 三层；每次测试内真实 LAN Origin/登录/会话/注销交换断言 ok）。两新对象均被 ALF 放行——**r00/ALF 本轮不是阻断项，无证据需要用户防火墙操作**；全程零系统/防火墙/权限修改。监听端口细节样本本轮未采到（monitor ps comm 匹配缺陷+窗口短于采样节奏，如实记录），LAN 结论依据六处测试 stdout 断言。
- R05-ENV-R00 保留"按二进制实例偶发"观察属性：历史 `9f748902…` 曾被拦，`43d95970…`/`cf9bce2f…`/`d57ea731…` 连续放行——不能写成永久解除；未来重链接实例若再被拦按台账逐实例登记。

### 13.4 RR3 缺口闭合与空间阻断时间线（如实）

- **F42–F54 全部独立 CLOSED**：F42（A-REVIEW-02）、F45（B-REVIEW-01）、F27-RR3/F46（C-F46-REVIEW-01+H-REVIEW-02）、F28-RR3（E-REVIEW-02/04）、F47/F48（H-REVIEW-02）、F49（I-REVIEW-01）、F50（J-REVIEW-02）、F51（[F51-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/F51-REVIEW-01/REVIEW.md)）、F52（[F52-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/F52-REVIEW-01/REVIEW.md)）、F53（[L-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/L-REVIEW-01/REVIEW.md)）、F54（[M-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/M-REVIEW-01/REVIEW.md)）。F51/F52/F53/F54 是 FINAL-01/02/03 暴露并修复的集成/时序/分类缺口，**三轮终审 FAIL 原样保留为历史**：FINAL-01（56 嵌套 .git 夹具→绑定器拒收）、FINAL-02（唯一 symlink 条目）、FINAL-03（全链绑定/checkpoint 首次全稳里程碑 + F53 flake 与 F54 46 叶分类失败两项必需遗留）。
- 空间阻断按时间线保留：G02 真实 ENOSPC 阻断（2026-10-07，N01 有效/N02 无效/N03–N16 未跑）为历史事实；总控 cargo clean（280.7GiB）+ 部分 RR2 tmp 回收（[TASK0 回执](../../../artifacts/rust-tauri/R05/RR3/TASK0/)）解除阻断；[G-REVIEW-03](../../../artifacts/rust-tauri/R05/RR3/G-REVIEW-03/REVIEW.md) 以冷缓存全量重编完整执行默认 N01–N16（16/16 fail-closed 点名+controls 绿+恢复绿+真实 shell exit=0）与 full R02/E5（N16 run-b 20/20 overall=PASS、E5 seal-family 分类 GREEN、Node verify 64,765 PASS、12 文件逐字节恢复）；两次无效轮（default16-01 宿主终止、default16-02 共享缓存污染被 control fail-closed）原样保留。G01 exit2/15 行历史协调失败不冲销。
- [E-REVIEW-04](../../../artifacts/rust-tauri/R05/RR3/E-REVIEW-04/REVIEW.md) 已独立 PASS 关闭 E03 文档轮（其截点 NOT_ACCEPTED 为当时真）；本轮 E04 消费 FINAL-04 真实结果另行回填，待全新 E-REVIEW-05。

### 13.5 本轮回填边界与下一步

- 本 E04 只改 E_BRIEF 所列 14 份 owned 文档中的 12 份（WORKER_MODEL_BOUNDARY.md、R05_INTERFACE_EVOLUTION.md 字节不变）；受保护语义输入（rust/、scripts/、lock、R05 四 TSV、SCOPE_MATRIX、R00–R02 权威表、stage maps 等）逐项前后相等证明见 [E-04 报告](../../../artifacts/rust-tauri/R05/RR3/E-04/REPORT.md)。E14 字节变化使完整候选摘要变化——不声称与 FINAL-04 被测候选全树相等，不冒充新 testedSha。
- Git：至今零暂存/零提交/零推送（FINAL-04 亲核）；未来真实回执按 HANDOFF `git_delivery_receipt_contract` 独立归档，本报告不预写。
- 下一步（交总控）：E-REVIEW-05 全新独立文档审查 → DELIVERY-FINAL-02/DELIVERY-REVIEW-01 → 总控按既有授权精确 Git 提交/推送；**R06 可开始执行**（读 [HANDOFF](R05_HANDOFF.json) 与 R06 任务书；R07 份额叶仍 REQUIRED，LIVE/平台延期按原登记不变）。
- 明确声明：本放行为离线规定范围接受；真实付费供应商/OAuth LIVE 与 Windows/Linux 平台义务仍按原登记延期，不因本翻正冒称无条件全产品完成。
"""

REVIEW_S = """

## RR3 E-04 当前独立结论索引（FINAL-04；本注记不自签审查）

**最新已完成正式阶段审查=RR3/FINAL-04，PASS。** 全新空历史独立终审者亲跑 §5.3 六条命令全部 exit=0，三层 verify-stage（R05/R04/R03）overall=PASS、stable=true、checkpoint 全稳、runner 全 PASS、testedSha=b3ac0e6a+真实工作树、失败清单空（[STAGE_REVIEW](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/STAGE_REVIEW.md)、[STRUCTURED_SUMMARY](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/STRUCTURED_SUMMARY.json)）。

包级独立结论链（全部 PASS，指针）：[A-REVIEW-02](../../../artifacts/rust-tauri/R05/RR3/A-REVIEW-02/REVIEW.md)（F42）、[B-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/B-REVIEW-01/REVIEW.md)（F45）、[C-F46-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/C-F46-REVIEW-01/REVIEW.md)+[H-REVIEW-02](../../../artifacts/rust-tauri/R05/RR3/H-REVIEW-02/REVIEW.md)（F27/F46/F47/F48 及受影响资源）、[I-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/I-REVIEW-01/REVIEW.md)（F49）、[J-REVIEW-02](../../../artifacts/rust-tauri/R05/RR3/J-REVIEW-02/REVIEW.md)（F50）、[G-REVIEW-03](../../../artifacts/rust-tauri/R05/RR3/G-REVIEW-03/REVIEW.md)（默认 16+full R02/E5，G02 空间阻断按时间线保留）、[F51-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/F51-REVIEW-01/REVIEW.md)、[F52-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/F52-REVIEW-01/REVIEW.md)、[L-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/L-REVIEW-01/REVIEW.md)（F53）、[M-REVIEW-01](../../../artifacts/rust-tauri/R05/RR3/M-REVIEW-01/REVIEW.md)（F54）、[E-REVIEW-04](../../../artifacts/rust-tauri/R05/RR3/E-REVIEW-04/REVIEW.md)（E03 文档轮）。D-REVIEW-01 定位/精确准备 PASS；其历史 r00 gate FAIL 由 FINAL-04 两新对象（cf9bce2f…/d57ea731…）6 次 LAN 实测通过解除，无用户防火墙操作证据。

历史 FAIL 全部保留为历史：FINAL-01/02/03（三连最终终审 FAIL：F51/F52 夹具、F53 flake、F54 分类，均已独立修复关闭）、RR2/FINAL-01（R05 5/7、R04 8/8 与 R03 15/15 checkpoint 不稳）、G01/G02、E-REVIEW-01（MF-E01/MF-E02，已由 E-REVIEW-02 关闭）、A-REVIEW-01、RR1 INDEPENDENT-9。本轮 E04 文档回填仅 SELF_CHECKED，另待全新 E-REVIEW-05；不预写 Git 提交回执（至今零暂存/零提交/零推送）。六元组与剩余延期边界（LIVE 最迟 R10、平台 R09/R10、raw npm 登记红、directed/E5 原许可）见 [R05_REPORT §13](R05_REPORT.md#rr3-current)。
"""

BLOCKERS_S = """

## 10. RR3 E-04 当前缺口（2026-10-08）

**stage_readiness=ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS / R06_READY=true（[RR3/FINAL-04](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/STAGE_REVIEW.md)）。RR3 无未关闭的必需缺口**：F42–F54 全部独立 CLOSED（§9 及更早各节的历史阻断均为截点事实，保留不改写）；G-REVIEW-03 默认 16+full R02/E5 包级 PASS（G02 空间阻断经总控 cargo clean 解除，阻断与解除均按时间线保留）；FINAL-01/02/03 历史 FAIL 已由 F51/F52/F53/F54 修复关闭。当前仅存以下登记项（原边界，无新增豁免）：

- **RR-BLK-CREDENTIALS（§1，延期）**：LIVE 真实供应商验证未授权，最迟 R10；负责人=用户（凭证/预算）。不影响已接受的离线范围。
- **平台验证缺口（§4，继承）**：Linux x86_64 继承原登记未复验、Windows 未验证（R09/R10）；macOS arm64 本轮全部真实执行。不因离线放行清零。
- **R05-ENV-R00（原 §2 同族，观察属性）**：按二进制实例偶发——历史 9f748902… 曾被 ALF 拦；FINAL-01/02/03 43d95970… 与 FINAL-04 cf9bce2f…/d57ea731…（CDHash 364514be…）连续放行，**本轮无证据需要用户防火墙操作，也不能写成永久解除**；未来重链接实例若再被拦按台账逐实例登记。
- **观察项（非阻断）**：I06 额外 worker-permission NOT_OBSERVED（沿 G-INTERRUPTION→G-REVIEW-03 结论如实保留，不补造）；raw npm 历史 candidate 登记红保持 registered-not-formal-green。
- **流程收口（非产品阻断）**：E-REVIEW-05 全新独立文档审查 → DELIVERY-FINAL-02/DELIVERY-REVIEW-01 → 总控按既有授权精确 Git 提交/推送并归档真实回执（截至本截点零暂存/零提交/零推送，FINAL-04 亲核，不预写）。

除此以外无其他已知阻塞。raw npm 红、directed/E5 原许可范围、LIVE/平台延期的完整边界见 [R05_REPORT §13](R05_REPORT.md#rr3-current)；负测状态见 [R05_NEGATIVE_GATE_REPORT](R05_NEGATIVE_GATE_REPORT.md) 的 RR3 E-04 节。
"""

NEG_S = """

## RR3 E-04 当前默认负测状态（2026-10-08）

**[G-REVIEW-03](../../../artifacts/rust-tauri/R05/RR3/G-REVIEW-03/REVIEW.md) 包级独立 PASS（无 mustFix）**：有效轮 default16-03 完整真实默认运行——N01–N16 **16/16 fail-closed 且逐项点名**、controls 绿（xtask 镜像 8/0/0/113 与 binary_wiring 2/0/0/0 全量真实重编）、N03 字节恢复后精确 1/1 绿、N06/N16 绑定先于变异+两次不同绑定 digest 且旧根拒收、**真实 shell exit=0 落盘**（G02 的 UNKNOWN 缺口补上，2026-10-07T10:59:20Z）、最终 12 文件 cp+cmp 逐字节恢复（独立 SHA 复算全等）。两次无效轮原样保留：default16-01 宿主终止（UNKNOWN）、default16-02 共享 NEG_TARGET 缓存污染被 control 即时 fail-closed（流程教训，非产品缺陷）。

- **full R02/full E5（G02 未达范围的覆盖）**：N16 run-a 19/20（a16 E5 全量 npm 真实跑、一次 vitest worker 崩溃被完备性谓词正确 fail-closed，环境偶发）；**N16 run-b 20/20 命令 overall=PASS，a16 E5 全量+seal-family 分类 GREEN**（候选红 ⊆ baseline replay 红 ∪ 登记预存族）；F47/F48/F50 修复后 a01/a13/a05_a06/三 CLI supplemental 多轮全 PASS；Node verify 64,765 项 PASS。
- **raw npm（如实）**：候选端 seal trio 3 文件/6 失败历史红**保持登记 registered-not-formal-green**，不写全绿、不扩大豁免；directed-no-seal-family E0–E4.5 原明确许可不变。
- 正式链内口径：FINAL-04 R03 层 r02_legacy_regression 为 directed（E0–E4.5 全绿、E5 BY SCOPE SKIP）；full E5 义务由上述 G-REVIEW-03 隔离副本独立证明（历史有效）。G01 exit2/15 行与 G02 ENOSPC 阻断保留为历史（空间解除时间线见 R05_REPORT §13.4）。
- 原 16A/100+3C/130 叶与全部原负测身份不变；受影响链复用按 [G-REVIEW-03 reuse-input-equality](../../../artifacts/rust-tauri/R05/RR3/G-REVIEW-03/metadata/reuse-input-equality.json) 输入相等边界执行（H02 375/375、A1 17、J 998/1016 差异全为已审 docs、I 20/21 唯一差为 J02 已新验准备段）。
"""

USAGE_L3_OLD = "版本：2026-10-07／RR3 当前源码核对（stage_readiness=NOT_ACCEPTED，R06_READY=false）。"
USAGE_L3_NEW = "版本：2026-10-08／RR3 当前源码核对（stage_readiness=ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS，R06_READY=true；RR3/FINAL-04 放行）。"
USAGE_TAIL_OLD = "H02 F48诊断/关联保护独立PASS不代替I09完整组合；I01–I09当前组合待验、I06额外worker-permission未观察、I11未完成。见[当前报告§12](R05_REPORT.md#rr3-current)与[HANDOFF](R05_HANDOFF.json)。本E只回填证据，不重跑产品，不自签独立PASS；R06_READY=false。"
USAGE_TAIL_NEW = "H02 F48诊断/关联保护独立PASS；I11由G-REVIEW-03默认16完整亲跑关闭、I01–I09完整组合由RR3/FINAL-04正常态全链亲跑补齐（workspace 115组1486全绿+r05_stage_suites全绿+R04/R03闭包PASS），I06额外worker-permission仍NOT_OBSERVED（观察项，如实保留）。见[当前报告§13](R05_REPORT.md#rr3-current)与[HANDOFF](R05_HANDOFF.json)。本E04只回填FINAL-04真实结果，不重跑产品，不自签独立PASS；R06_READY=true（离线范围，LIVE/平台延期原边界）。"


def rw(rel):
    p = os.path.join(REPO, rel)
    with open(p, encoding="utf-8") as fh:
        return p, fh.read()


def patch(rel, replacements, banner=None, banner_prefix="> **RR3 E-03", append=None):
    p, text = rw(rel)
    orig = text
    if banner:
        # 整行替换以 banner_prefix 开头的首行引用块
        lines = text.split("\n")
        idx = next(i for i, ln in enumerate(lines) if ln.startswith(banner_prefix))
        # 该引用块可能占一行（E-03 各文件的 banner 均为单行长引用）
        lines[idx] = banner
        text = "\n".join(lines)
    for old, new in replacements:
        assert text.count(old) == 1, f"{rel}: pattern count {text.count(old)} != 1 for {old[:60]!r}"
        text = text.replace(old, new)
    if append:
        if not text.endswith("\n"):
            text += "\n"
        text += append
    with open(p, "w", encoding="utf-8") as fh:
        fh.write(text)
    print(f"patched {rel} ({len(orig)} -> {len(text)} chars)")


def main():
    # 1) 主报告
    patch(
        "docs/rust-tauri/R05/R05_REPORT.md",
        [
            ("# R05_REPORT — RR3 E-03当前真实阻断与交接（§12；历史保留）",
             "# R05_REPORT — RR3 E-04 FINAL-04放行状态回填（§13；历史保留）"),
            ("§10 由 R05 RR2 收口（WP-F）于 2026-10-06/07 增补——§1–§10均为RR1/RR2历史原文；E-02当时当前指针为§11；本轮生成截点以§12及HANDOFF rr3_current为准。",
             "§10 由 R05 RR2 收口（WP-F）于 2026-10-06/07 增补——§1–§10均为RR1/RR2历史原文；E-02当时当前指针为§11；E-03截点指针为§12；本轮（E-04）生成截点以§13及HANDOFF rr3_current为准。"),
            ('<a id="rr3-current"></a>\n## 12. RR3 E-03 当前状态与交接（2026-10-07）',
             '## 12. RR3 E-03 当前状态与交接（2026-10-07；已由§13取代，历史截点）'),
        ],
        banner=BANNER,
        append=REPORT_S13,
    )
    # 2) 独立结论索引
    patch(
        "docs/rust-tauri/R05/R05_INDEPENDENT_REVIEW.md",
        [
            ("## RR3 E-03 当前独立结论索引（本注记不自签审查）",
             "## RR3 E-03 历史独立结论索引（本注记不自签审查；已由下方 E-04 索引取代）"),
        ],
        banner=BANNER,
        append=REVIEW_S,
    )
    # 3) 阻断
    patch(
        "docs/rust-tauri/R05/R05_BLOCKERS.md",
        [
            ("## 9. RR3 E-03 当前必需阻断（2026-10-07）",
             "## 9. RR3 E-03 当前必需阻断（2026-10-07；已由§10取代，历史截点）"),
        ],
        banner=BANNER,
        append=BLOCKERS_S,
    )
    # 4) 负测报告
    patch(
        "docs/rust-tauri/R05/R05_NEGATIVE_GATE_REPORT.md",
        [
            ("## RR3 E-03 当前默认负测状态（2026-10-07）",
             "## RR3 E-03 当前默认负测状态（2026-10-07；已由 E-04 节取代，历史截点）"),
        ],
        banner=BANNER,
        append=NEG_S,
    )
    # 5) usage 说明（仅状态指针）
    patch(
        "docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md",
        [
            (USAGE_L3_OLD, USAGE_L3_NEW),
            (USAGE_TAIL_OLD, USAGE_TAIL_NEW),
        ],
    )


if __name__ == "__main__":
    main()
