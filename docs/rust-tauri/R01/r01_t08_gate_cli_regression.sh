#!/usr/bin/env bash
# R01-T08 关卡真实 CLI 负向回归（R01 阶段验收 R2/R3 F01）。
#
# r01_t08_gate_check.py --self-test 覆盖进程内 evaluate() 负向；本脚本以真实
# 命令行入口（python3 -B r01_t08_gate_check.py --register <变体>）复验同一组
# 风险登记攻击变体全部被拒（exit 1 且输出对应 BLOCKED 类别），并验证真实
# 登记正向通过（exit 0）——防止"只调用内部函数"层面之外的 CLI 装配回归。
# R3 追加截止阶段绑定变体：不存在的阶段（R99）/含糊措辞/过晚截止期/越过
# 最迟关卡/缺结构化坐标/坐标倒挂，对应 G6 七类拒绝。
# R4 追加完整标识边界变体（G6b）：正文 R099（旧 R\d{2} 截成 R09 放行的原样
# 反例）/混合 R09/R099/前缀伪装 XR09/后缀伪装 R09X/最迟标记 R099/四位 R0999，
# 完整形态提取 + 伪装检出后必须全部 exit 1。
# R5 追加复合/小阶段/拼接形态变体（G6b 边界扩展）：R4 边界类 [A-Za-z0-9_]
# 把点号/连字符当合法边界，正文 R09.5/R09-5/R09．5 被截成 R09 放行（R5 评审
# 实测 exit 0）；R5 将点号/连字符族列入标识延续字符，这类出现按完整复合形态
# 报 risk-stage-disguised——R09.5/R09-5/R09．5/R09－5/R09.5.1/R09.R10/最迟
# R09.5/混合 R09/R09.5 必须全部 exit 1。
# R6 追加全角同形/大小写形态变体（G6c 全角折叠）：R5 及以前三条正则只从 ASCII
# R 开始，全角同形阶段在正文首位或「最迟」后完全隐身（R6 评审实测 Ｒ９９…；R09
# 与 R09…最迟 Ｒ１０ 均 exit 0/PASS_WITH_CONDITIONS）；R6 在提取前 1:1 折叠全角
# 拉丁字母/数字为 ASCII，伪装检出带 IGNORECASE——正文首位 Ｒ９９/最迟 Ｒ１０/
# 正文首位 Ｒ１０/混合宽度 Ｒ99/全角复合 ＸＲ０９/小写 r99/全角小写 ｒ９９ 必须
# 全部 exit 1；另有规范化正向：正文 Ｒ０９（全角）与结构化 R09 一致时 exit 0
# （方案 a 先规范化再检验，不误伤一致的全角书写）。
# R7 追加重复「最迟」逐处核验变体（G6d）：R6 及以前 LATEST_MARKER_RE.search()
# 只校验第一处「最迟」，正文「最迟 R09；最迟 R10」（结构化最迟 R09）的第二处
# 相矛盾期限被完全忽略（R7 评审实测 exit 0 / PASS_WITH_CONDITIONS）；R7 修复为
# finditer 逐处核验——首处一致/后处冲突、顺序交换、全角混用三变体必须全部
# exit 1；另有两个正向：多处「最迟 R09」全部一致 exit 0（清晰契约放行）、正常
# 上下文提及 R10（非最迟标记）不误伤 exit 0。
# R8 追加显式期限连接形态变体（G6d 连接形态扩展）：R7 标记语法只识别「最迟」
# 后接空白再紧跟阶段 ID，「最迟：R10」（全角冒号）/「最迟：Ｒ１０」（全角冒号+
# 全角阶段）/「最迟于 R10」（连接词）/「最迟: R10」（半角冒号）/「最迟为 R10」
# （连接词族）/「不晚于 R10」（同族否定连接式）是同样明确的期限声明却完全不
# 构成标记（R8 评审实测 exit 0 / PASS_WITH_CONDITIONS 误放，绕过 R7 逐处核验）；
# 「最迟于第三阶段完成」的触发词解析不出阶段 ID，按 risk-stage-latest-unresolved
# 失败关闭——八变体必须全部 exit 1；另有两个正向：连接形态一致「最迟：R09；
# 最迟于 R09」与否定连接式一致「不晚于 R09」均 exit 0。
# R9 追加单个期限子句内第二阶段变体（G6e 期限子句唯一一致）：R8 标记语法把
# 触发词绑定到其后首个阶段 ID，同一子句内首个阶段之后的第二阶段——改期
# 「最迟（原定 R09，现改 R10）完成」/顺延「最迟由原定的 R09 顺延至 R10 完成」/
# 选择「最迟在 R09 或 R10 完成」/斜杠并列「最迟 R09/R10 完成」及全角变体
# （全角改期「（原定 Ｒ０９，现改 Ｒ１０）」/全角斜杠「Ｒ０９／Ｒ１０」/全角
# 选择「Ｒ０９ 或 Ｒ１０」）与拉丁连接「or」——被通用阶段扫描当普通上下文只查
# 存在性，明确改晚的强制期限仍 exit 0 误放（R9 评审实测，结构化坐标保持
# R09/R09）；R9 修复为「每个触发词治理从触发词起到子句终结符止的完整区域，
# 区域内每个完整阶段引用都必须与结构化最迟一致」——不枚举改期/顺延/选择
# 语言，任何第二阶段统一覆盖，八变体必须全部 exit 1；另有两个正向：同子句内
# 重复一致「最迟 R09 完成（R09 复核）」与引号词名引用「R10 文档介绍“最迟”
# 字段」（成对引号紧包的触发词非期限声明，闭引号紧邻触发词、不可能携带阶段
# ID，豁免无法隐藏期限）均 exit 0。
# R10 追加期限区域完整性变体（G6f，R10 评审实测原样反例）：R9 的引号紧包豁免
# 实际跳过整个触发词（解析义务+区域治理一并移除），词名之后的实际赋值/改期
# 「“最迟”原定 R09，现改 R10 完成」「“最迟”字段：原定 R09，现改 R10 完成」
# （“”/「」/『』/ASCII " 四种支持引号同族）与非唯一期限「“最迟” R09/R10 完成」
# 「「最迟」 Ｒ０９／Ｒ１０ 完成」均 exit 0 误放；区域终结符不看括号层次，
# 「最迟（原定 R09；现改 R10）完成」及括号内换行同形截断完整改期声明。R10
# 评审另独立裁定：「最晚于 R10 完成」「R10 前必须完成」等已披露明确截止不得
# 凭「文档化边界」自动当普通上下文（R9 修复报告称其为边界正向——本轮按评审
# 裁定翻转为负向；其「任务书明确禁止同义词枚举」的说法在原任务书中无原文
# 依据，已更正：「最晚」系迟/晚对称的封闭形态族补全，非开放枚举）。R10 修复：
# 词名豁免收窄为仅解析义务、区域边界括号感知、「最晚」入触发词族逐处核验、
# 阶段锚定「之前/以前/前」边界期限按 risk-stage-boundary-unsupported 失败关闭
# ——十二变体必须全部 exit 1；另有三个正向：直角引号词名引用「R10 文档介绍
# 「最迟」字段」、同义词一致「最晚于 R09 完成」与词名后一致提及「“最迟”字段
# 即 R09」均 exit 0。
#
# 用法：bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh
# 退出码：0 = 全部按预期；1 = 任一变体未被拒绝或正向对照失败。
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/r01_t08_gate_check.py"
REGISTER="$HERE/RISK_REGISTER.json"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# 变体改写：全部形态硬编码枚举（不拼接、不 eval 外部输入）。
apply_variant() { # $1=变体名 $2=目标登记路径
  python3 - "$1" "$2" <<'PYEOF'
import json, sys
name, path = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as fh:
    reg = json.load(fh)

def entry(rid="RR-T05-X1"):
    return next(x for x in reg["risks"] if x["id"] == rid)

if name == "null-field":
    entry()["resolve_by_stage"] = None
elif name == "object-field":
    entry()["resolve_by_stage"] = {}
elif name == "array-field":
    entry()["failure_handling"] = []
elif name == "blank-field":
    entry()["resolve_by_stage"] = "  \n "
elif name == "number-field":
    entry()["resolve_by_stage"] = 123
elif name == "bool-field":
    entry()["failure_handling"] = True
elif name == "placeholder-field":
    entry()["failure_handling"] = "tbd"
elif name == "deleted-field":
    entry().pop("resolve_by_stage", None)
elif name == "closed-status":
    entry()["status"] = "CLOSED"
elif name == "closed-in-r01-status":
    entry()["status"] = "CLOSED_IN_R01"
elif name == "defect-status-mismatch":
    entry("RR-T07-PROD-DEFECT-1")["status"] = "OPEN"
elif name == "documented-status-mismatch":
    entry("RR-T07-F2")["status"] = "OPEN"
elif name == "duplicate-id":
    reg["risks"].append(dict(reg["risks"][0]))
elif name == "missing-id":
    reg["risks"].append({"status": "OPEN", "resolve_by_stage": "R09", "failure_handling": "x"})
elif name == "risks-non-list":
    reg["risks"] = {}
elif name == "stage-text-nonexistent":
    # R01 阶段验收 R3 F01 原样反例：截止阶段正文指向不存在的 R99。
    entry()["resolve_by_stage"] = "R99（不存在的阶段）"
elif name == "stage-both-nonexistent":
    x = entry()
    x["resolve_by_stage"] = "R99"
    x["resolve_by_stage_id"] = "R99"
elif name == "stage-vague-text":
    entry()["resolve_by_stage"] = "以后再说"
elif name == "stage-deadline-too-late":
    x = entry("RR-T02-F1")
    x["resolve_by_stage"] = "R10（拖延）"
    x["resolve_by_stage_id"] = "R10"
    x["resolve_latest_stage_id"] = "R10"
elif name == "stage-latest-past-gate":
    entry("RR-T06-MIC")["resolve_latest_stage_id"] = "R10"
elif name == "stage-field-missing":
    entry().pop("resolve_by_stage_id", None)
elif name == "stage-order-inverted":
    entry("RR-T07-F2")["resolve_latest_stage_id"] = "R07"
elif name == "stage-text-r099":
    # R01 阶段验收 R4 F01 原样反例：正文 R099 在旧 R\d{2} 下被截成 R09 放行。
    entry()["resolve_by_stage"] = "R099（不存在的阶段）"
elif name == "stage-text-mixed-r09-r099":
    entry()["resolve_by_stage"] = "R09/R099（混合了伪造阶段）"
elif name == "stage-text-prefixed-xr09":
    entry()["resolve_by_stage"] = "XR09（伪造阶段）"
elif name == "stage-text-suffixed-r09x":
    entry()["resolve_by_stage"] = "R09X（后缀伪装阶段）"
elif name == "stage-latest-marker-r099":
    entry()["resolve_by_stage"] = "R09 最迟 R099"
elif name == "stage-text-r0999":
    entry()["resolve_by_stage"] = "R0999（多一位伪装阶段）"
elif name == "stage-text-dot-substage":
    # R5 评审原样反例：正文 R09.5 在 R4 边界类下被截成 R09 放行。
    entry()["resolve_by_stage"] = "R09.5（虚构小阶段）"
elif name == "stage-text-hyphen-substage":
    entry()["resolve_by_stage"] = "R09-5（虚构小阶段）"
elif name == "stage-text-fullwidth-dot-substage":
    entry()["resolve_by_stage"] = "R09．5（虚构小阶段）"
elif name == "stage-text-fullwidth-hyphen-substage":
    entry()["resolve_by_stage"] = "R09－5（虚构小阶段）"
elif name == "stage-text-multi-level-substage":
    entry()["resolve_by_stage"] = "R09.5.1（多级虚构小阶段）"
elif name == "stage-text-concat-two-stages":
    entry()["resolve_by_stage"] = "R09.R10（点号拼接阶段标识）"
elif name == "stage-latest-marker-dot-substage":
    entry()["resolve_by_stage"] = "R09 最迟 R09.5"
elif name == "stage-text-mixed-real-compound":
    entry()["resolve_by_stage"] = "R09/R09.5（混合真实阶段与虚构小阶段）"
elif name == "stage-text-fullwidth-r99-first":
    # R6 评审原样反例 1：正文首位全角 Ｒ９９（不存在的截止阶段）与合法 R09 混用。
    entry()["resolve_by_stage"] = "Ｒ９９（不存在的截止阶段）；R09（仅用于宿主集成）"
elif name == "stage-text-latest-fullwidth-r10":
    # R6 评审原样反例 2：「最迟 Ｒ１０」全角虚构放宽关卡。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 Ｒ１０（虚构放宽关卡）"
elif name == "stage-text-fullwidth-r10-first":
    entry()["resolve_by_stage"] = "Ｒ１０（虚构放宽关卡）；R09（宿主集成）"
elif name == "stage-text-fullwidth-r-ascii-digit":
    entry()["resolve_by_stage"] = "Ｒ99（不存在的截止阶段）；R09（校验）"
elif name == "stage-text-fullwidth-compound-xr09":
    entry()["resolve_by_stage"] = "ＸＲ０９（全角前缀伪装）；R09（校验）"
elif name == "stage-text-lowercase-r99":
    entry()["resolve_by_stage"] = "r99（不存在的截止阶段）；R09（仅用于宿主集成）"
elif name == "stage-text-fullwidth-lowercase-r99":
    entry()["resolve_by_stage"] = "ｒ９９（不存在的截止阶段）；R09（校验）"
elif name == "stage-text-fullwidth-normalized-positive":
    # R6 规范化正向：全角 Ｒ０９ 折叠后与结构化 R09 一致，不得误伤。
    entry()["resolve_by_stage"] = "Ｒ０９（看似合法的截止）；R09（校验）"
elif name == "stage-latest-double-conflict":
    # R7 评审原样反例：首处一致/后处冲突——R6 及以前 search() 只查首处「最迟」，
    # 第二处「最迟 R10」与结构化最迟 R09 相矛盾却被忽略放行。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）"
elif name == "stage-latest-double-reversed":
    # R7 方向性对照：顺序交换——首处「最迟 R10」即与结构化 R09 矛盾。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R10；最迟 R09"
elif name == "stage-latest-double-fullwidth-mixed":
    # R7 全角混用：折叠后第二处「最迟 R10」与结构化 R09 矛盾。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 Ｒ０９；最迟 Ｒ１０"
elif name == "stage-latest-double-consistent":
    # R7 多处一致正向：两处「最迟 R09」均与结构化 R09 一致，清晰契约放行。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R09；最迟 R09（重复但一致）"
elif name == "stage-latest-contextual-r10":
    # R7 不误伤正向：R10 是与「最迟」语法无关的正常上下文提及（真实存在的阶段），
    # 唯一最迟标记 R09 与结构化一致。
    entry()["resolve_by_stage"] = "R09（宿主集成）；R10（后续平台事项）最迟 R09"
elif name == "stage-latest-fullwidth-colon-conflict":
    # R8 评审原样反例：全角冒号连接的第二处期限「最迟：R10」——R7 标记语法只认
    # 「最迟 R10」，冒号/连接词形态完全不构成标记，曾被 exit 0 误放。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R09；最迟：R10（虚构放宽关卡）"
elif name == "stage-latest-fullwidth-colon-fullwidth-stage":
    # R8 评审原样反例：全角冒号 + 全角阶段「最迟：Ｒ１０」。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R09；最迟：Ｒ１０（虚构放宽关卡）"
elif name == "stage-latest-yu-conflict":
    # R8 评审原样反例：连接词「最迟于 R10」。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R09；最迟于 R10（虚构放宽关卡）"
elif name == "stage-latest-single-colon-conflict":
    # R8 评审原样反例：单处冒号形式（无第一处一致掩护）。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟：R10（虚构放宽关卡）"
elif name == "stage-latest-halfwidth-colon-conflict":
    # R8 变体：半角冒号「最迟: R10」。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟: R10（虚构放宽关卡）"
elif name == "stage-latest-wei-conflict":
    # R8 变体：连接词族代表「最迟为 R10」（有界连接段覆盖非枚举连接词）。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟为 R10（虚构放宽关卡）"
elif name == "stage-latest-not-later-than-conflict":
    # R8 变体：同族否定连接式「不晚于 R10」——与「最迟」同为显式期限声明。
    entry()["resolve_by_stage"] = "R09（宿主集成）不晚于 R10（虚构放宽关卡）"
elif name == "stage-latest-unresolved-trigger":
    # R8 变体（失败关闭）：触发词「最迟」在子句内解析不出阶段 ID（自然语言期限）。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟于第三阶段完成"
elif name == "stage-latest-connector-forms-consistent":
    # R8 连接形态一致正向：全角冒号/连接词形态的多处「最迟 R09」均一致，放行。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟：R09；最迟于 R09（重复但一致）"
elif name == "stage-latest-negative-trigger-consistent":
    # R8 否定连接式一致正向：「不晚于 R09」与结构化一致，放行。
    entry()["resolve_by_stage"] = "R09（宿主集成）不晚于 R09 完成"
elif name == "stage-latest-clause-correction":
    # R9 评审原样反例 1：改期——「最迟（原定 R09，现改 R10）完成」。R8 标记语法
    # 把触发词绑定到首个 R09，同一子句内的「现改 R10」被当普通上下文只查存在性。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟（原定 R09，现改 R10）完成"
elif name == "stage-latest-clause-postponed":
    # R9 评审原样反例 2：顺延——「最迟由原定的 R09 顺延至 R10 完成」。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟由原定的 R09 顺延至 R10 完成"
elif name == "stage-latest-clause-or-choice":
    # R9 变体：选择——「最迟在 R09 或 R10 完成」，可选择更晚期限即非唯一期限。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟在 R09 或 R10 完成"
elif name == "stage-latest-clause-slash-pair":
    # R9 变体：斜杠并列——「最迟 R09/R10 完成」，非唯一期限被首阶段代表。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R09/R10 完成"
elif name == "stage-latest-clause-fullwidth-correction":
    # R9 变体：全角改期——「最迟（原定 Ｒ０９，现改 Ｒ１０）完成」，折叠后同规则。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟（原定 Ｒ０９，现改 Ｒ１０）完成"
elif name == "stage-latest-clause-fullwidth-slash":
    # R9 变体：全角斜杠并列——「最迟 Ｒ０９／Ｒ１０ 完成」（／为合法分隔符）。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 Ｒ０９／Ｒ１０ 完成"
elif name == "stage-latest-clause-fullwidth-or":
    # R9 变体：全角阶段选择——「最迟在 Ｒ０９ 或 Ｒ１０ 完成」。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟在 Ｒ０９ 或 Ｒ１０ 完成"
elif name == "stage-latest-clause-latin-or":
    # R9 变体：拉丁连接——「最迟在 R09 or R10 完成」，区域规则语言无关。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟在 R09 or R10 完成"
elif name == "stage-latest-clause-repeat-consistent":
    # R9 同子句内重复一致正向：触发词区域内重复阶段引用全部等于结构化 latest，
    # 唯一一致期限可判定，放行。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟 R09 完成（R09 复核）"
elif name == "stage-latest-quoted-fieldname":
    # R9 引号词名引用正向（R9 评审披露限制的最小处理）：被成对引号紧包的「“最迟”」
    # 是引用词名而非期限声明；R10 为终结符后普通上下文提及。
    entry()["resolve_by_stage"] = "R09 最迟 R09 完成；R10 文档介绍“最迟”字段"
elif name == "stage-latest-quote-wordname-correction":
    # R10 评审原样反例：引号词名后实际改期——词名引用不遮蔽其后续区域的期限书写。
    entry()["resolve_by_stage"] = "R09（宿主集成）“最迟”原定 R09，现改 R10 完成"
elif name == "stage-latest-quote-fieldname-correction":
    # R10 评审原样反例：字段名引用后实际赋值/改期。
    entry()["resolve_by_stage"] = "R09（宿主集成）“最迟”字段：原定 R09，现改 R10 完成"
elif name == "stage-latest-quote-fieldname-corner":
    # R10 quote-pairs：直角引号「」字段名同形。
    entry()["resolve_by_stage"] = "R09（宿主集成）「最迟」字段：原定 R09，现改 R10 完成"
elif name == "stage-latest-quote-fieldname-double-corner":
    # R10 quote-pairs：双层直角引号『』字段名同形。
    entry()["resolve_by_stage"] = "R09（宿主集成）『最迟』字段：原定 R09，现改 R10 完成"
elif name == "stage-latest-quote-fieldname-ascii":
    # R10 quote-pairs：ASCII 双引号字段名同形。
    entry()["resolve_by_stage"] = "R09（宿主集成）\"最迟\"字段：原定 R09，现改 R10 完成"
elif name == "stage-latest-quote-wordname-slash-pair":
    # R10 评审原样反例：词名引用后的斜杠非唯一期限。
    entry()["resolve_by_stage"] = "R09（宿主集成）“最迟” R09/R10 完成"
elif name == "stage-latest-quote-wordname-fw-slash":
    # R10 评审原样反例：词名引用后的全角斜杠非唯一期限（折叠后同规则）。
    entry()["resolve_by_stage"] = "R09（宿主集成）「最迟」 Ｒ０９／Ｒ１０ 完成"
elif name == "stage-latest-paren-semicolon-correction":
    # R10 评审原样反例：括号内分号截断完整改期声明——区域边界须括号感知。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟（原定 R09；现改 R10）完成"
elif name == "stage-latest-paren-newline-correction":
    # R10 评审原样反例：括号内换行截断完整改期声明（同根因区域切断）。
    entry()["resolve_by_stage"] = "R09（宿主集成）最迟（原定 R09\n现改 R10）完成"
elif name == "stage-latest-synonym-conflict":
    # R10 评审裁定边界 1：「最晚」入触发词族（迟/晚对称封闭形态族），
    # 「最晚于 R10」≠ 结构化最迟 R09 即拒——已披露明确截止不得当普通上下文。
    entry()["resolve_by_stage"] = "R09（宿主集成）；最晚于 R10 完成"
elif name == "stage-before-suffix-conflict":
    # R10 评审裁定边界 2：阶段锚定「前」边界期限「R10 前必须完成」——机器契约
    # 不支持 before-stage 语义核验，失败关闭拒绝（R9 曾记文档化边界正向，翻转）。
    entry()["resolve_by_stage"] = "R09（宿主集成）；R10 前必须完成"
elif name == "stage-before-suffix-variant":
    # R10 泛化对照：「之前」同族前边界期限（封闭词素族统一覆盖，非字面黑名单）。
    entry()["resolve_by_stage"] = "R09（宿主集成）；R10 之前完成"
elif name == "stage-latest-quoted-fieldname-corner":
    # R10 直角引号词名引用正向：「」紧包的词名引用非期限声明，仅豁免解析义务；
    # 其后续区域「「最迟」字段」无阶段引用，不构成遮蔽。
    entry()["resolve_by_stage"] = "R09 最迟 R09 完成；R10 文档介绍「最迟」字段"
elif name == "stage-latest-synonym-consistent":
    # R10 同义词一致正向：「最晚于 R09」与结构化 latest R09 一致即放行，
    # 入族不误伤一致书写。
    entry()["resolve_by_stage"] = "R09（宿主集成）最晚于 R09 完成"
elif name == "stage-latest-quoted-consistent-mention":
    # R10 词名后一致提及正向：词名引用后续区域受治理，区域引用与结构化一致
    # （R09 == R09）即放行——豁免只针对「不一致」期限的遮蔽，不误伤一致书写。
    entry()["resolve_by_stage"] = "R09（宿主集成）“最迟”字段即 R09"
else:
    sys.exit(f"unknown variant {name}")
with open(path, "w", encoding="utf-8") as fh:
    json.dump(reg, fh, ensure_ascii=False)
PYEOF
}

fail=0

# C0 正向对照：真实登记必须 exit 0 且 PASS_WITH_CONDITIONS。
out="$(python3 -B "$CHECK" --register "$REGISTER" 2>&1)"
rc=$?
if [ $rc -eq 0 ] && printf '%s' "$out" | grep -q "VERDICT: PASS_WITH_CONDITIONS"; then
  echo "PASS C0-cli-positive-control: exit=0 verdict=PASS_WITH_CONDITIONS"
else
  echo "FAIL C0-cli-positive-control: exit=$rc"
  printf '%s\n' "$out" | tail -5
  fail=1
fi

# 负向电池：每个变体必须 exit 1 且输出期望的拒绝类别文本。
run_negative() { # $1=变体名 $2=期望输出文本
  cp "$REGISTER" "$TMP/reg.json"
  apply_variant "$1" "$TMP/reg.json" || { echo "FAIL $1: 变体构造失败"; fail=1; return; }
  out="$(python3 -B "$CHECK" --register "$TMP/reg.json" 2>&1)"
  rc=$?
  if [ $rc -eq 1 ] && printf '%s' "$out" | grep -qF "$2"; then
    echo "PASS-NEG cli-$1: exit=1 含 '$2'"
  else
    echo "FAIL cli-$1: exit=${rc}（期望 exit=1 且含 '$2'）"
    printf '%s\n' "$out" | tail -8
    fail=1
  fi
}

run_negative null-field                  "risk-incomplete"
run_negative object-field                "risk-incomplete"
run_negative array-field                 "risk-incomplete"
run_negative blank-field                 "risk-incomplete"
run_negative number-field                "risk-incomplete"
run_negative bool-field                  "risk-incomplete"
run_negative placeholder-field           "risk-incomplete"
run_negative deleted-field               "risk-incomplete"
run_negative closed-status               "risk-status-contradiction"
run_negative closed-in-r01-status        "risk-status-contradiction"
run_negative defect-status-mismatch      "risk-status-contradiction"
run_negative documented-status-mismatch  "risk-status-contradiction"
run_negative duplicate-id                "重复"
run_negative missing-id                  "缺合法 id"
run_negative risks-non-list              "非列表"
run_negative stage-text-nonexistent      "risk-stage-unknown"
run_negative stage-both-nonexistent      "risk-stage-unknown"
run_negative stage-vague-text            "risk-stage-vague"
run_negative stage-deadline-too-late     "risk-stage-deadline-mismatch"
run_negative stage-latest-past-gate      "risk-stage-latest-mismatch"
run_negative stage-field-missing         "risk-stage-field-missing"
run_negative stage-order-inverted        "risk-stage-order-inverted"
run_negative stage-text-r099             "risk-stage-unknown"
run_negative stage-text-mixed-r09-r099   "risk-stage-unknown"
run_negative stage-text-prefixed-xr09    "risk-stage-disguised"
run_negative stage-text-suffixed-r09x    "risk-stage-disguised"
run_negative stage-latest-marker-r099    "risk-stage-unknown"
run_negative stage-text-r0999            "risk-stage-unknown"
run_negative stage-text-dot-substage           "risk-stage-disguised"
run_negative stage-text-hyphen-substage        "risk-stage-disguised"
run_negative stage-text-fullwidth-dot-substage "risk-stage-disguised"
run_negative stage-text-fullwidth-hyphen-substage "risk-stage-disguised"
run_negative stage-text-multi-level-substage   "risk-stage-disguised"
run_negative stage-text-concat-two-stages      "risk-stage-disguised"
run_negative stage-latest-marker-dot-substage  "risk-stage-disguised"
run_negative stage-text-mixed-real-compound    "risk-stage-disguised"
run_negative stage-text-fullwidth-r99-first         "risk-stage-unknown"
run_negative stage-text-latest-fullwidth-r10        "risk-stage-text-mismatch"
run_negative stage-text-fullwidth-r10-first         "risk-stage-text-mismatch"
run_negative stage-text-fullwidth-r-ascii-digit     "risk-stage-unknown"
run_negative stage-text-fullwidth-compound-xr09     "risk-stage-disguised"
run_negative stage-text-lowercase-r99               "risk-stage-disguised"
run_negative stage-text-fullwidth-lowercase-r99     "risk-stage-disguised"
run_negative stage-latest-double-conflict           "risk-stage-text-mismatch"
run_negative stage-latest-double-reversed           "risk-stage-text-mismatch"
run_negative stage-latest-double-fullwidth-mixed    "risk-stage-text-mismatch"
run_negative stage-latest-fullwidth-colon-conflict           "risk-stage-text-mismatch"
run_negative stage-latest-fullwidth-colon-fullwidth-stage   "risk-stage-text-mismatch"
run_negative stage-latest-yu-conflict                       "risk-stage-text-mismatch"
run_negative stage-latest-single-colon-conflict             "risk-stage-text-mismatch"
run_negative stage-latest-halfwidth-colon-conflict          "risk-stage-text-mismatch"
run_negative stage-latest-wei-conflict                      "risk-stage-text-mismatch"
run_negative stage-latest-not-later-than-conflict           "risk-stage-text-mismatch"
run_negative stage-latest-unresolved-trigger                "risk-stage-latest-unresolved"
run_negative stage-latest-clause-correction                 "risk-stage-text-mismatch"
run_negative stage-latest-clause-postponed                  "risk-stage-text-mismatch"
run_negative stage-latest-clause-or-choice                  "risk-stage-text-mismatch"
run_negative stage-latest-clause-slash-pair                 "risk-stage-text-mismatch"
run_negative stage-latest-clause-fullwidth-correction       "risk-stage-text-mismatch"
run_negative stage-latest-clause-fullwidth-slash            "risk-stage-text-mismatch"
run_negative stage-latest-clause-fullwidth-or               "risk-stage-text-mismatch"
run_negative stage-latest-clause-latin-or                   "risk-stage-text-mismatch"
run_negative stage-latest-quote-wordname-correction         "risk-stage-text-mismatch"
run_negative stage-latest-quote-fieldname-correction        "risk-stage-text-mismatch"
run_negative stage-latest-quote-fieldname-corner            "risk-stage-text-mismatch"
run_negative stage-latest-quote-fieldname-double-corner     "risk-stage-text-mismatch"
run_negative stage-latest-quote-fieldname-ascii             "risk-stage-text-mismatch"
run_negative stage-latest-quote-wordname-slash-pair         "risk-stage-text-mismatch"
run_negative stage-latest-quote-wordname-fw-slash           "risk-stage-text-mismatch"
run_negative stage-latest-paren-semicolon-correction        "risk-stage-text-mismatch"
run_negative stage-latest-paren-newline-correction          "risk-stage-text-mismatch"
run_negative stage-latest-synonym-conflict                  "risk-stage-text-mismatch"
run_negative stage-before-suffix-conflict                   "risk-stage-boundary-unsupported"
run_negative stage-before-suffix-variant                    "risk-stage-boundary-unsupported"

# R6/R7 正向组：一致书写不得被误伤——全角 Ｒ０９ 规范化一致（方案 a：先规范化再
# 检验）；多处「最迟 R09」全部一致（G6d 清晰契约放行）；正常上下文提及 R10
# （非「最迟」标记）不误伤。三者均须 exit 0 / PASS_WITH_CONDITIONS。
run_positive() { # $1=变体名 $2=说明
  cp "$REGISTER" "$TMP/reg.json"
  apply_variant "$1" "$TMP/reg.json" || { echo "FAIL $1: 变体构造失败"; fail=1; return; }
  out="$(python3 -B "$CHECK" --register "$TMP/reg.json" 2>&1)"
  rc=$?
  if [ $rc -eq 0 ] && printf '%s' "$out" | grep -q "VERDICT: PASS_WITH_CONDITIONS"; then
    echo "PASS cli-$1: exit=0 PASS_WITH_CONDITIONS（$2）"
  else
    echo "FAIL cli-$1: exit=${rc}（期望 exit=0 且 PASS_WITH_CONDITIONS）"
    printf '%s\n' "$out" | tail -8
    fail=1
  fi
}
run_positive stage-text-fullwidth-normalized-positive "全角规范化一致正向"
run_positive stage-latest-double-consistent           "多处最迟一致放行正向（G6d）"
run_positive stage-latest-contextual-r10              "正常上下文提及 R10 不误伤正向（G6d）"
run_positive stage-latest-connector-forms-consistent  "连接形态一致放行正向（G6d R8）"
run_positive stage-latest-negative-trigger-consistent "否定连接式一致放行正向（G6d R8）"
run_positive stage-latest-clause-repeat-consistent    "同子句内重复一致放行正向（G6e R9）"
run_positive stage-latest-quoted-fieldname            "引号词名引用不误拒正向（G6e R9）"
run_positive stage-latest-quoted-fieldname-corner     "直角引号词名引用不误拒正向（G6f R10）"
run_positive stage-latest-synonym-consistent          "同义词一致放行正向（G6f R10「最晚」入族）"
run_positive stage-latest-quoted-consistent-mention   "词名后一致提及放行正向（G6f R10）"

if [ $fail -eq 0 ]; then
  echo "CLI-REGRESSION OK（11 正向 + 74 负向）"
else
  echo "CLI-REGRESSION FAILED"
fi
exit $fail
