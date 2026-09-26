#!/usr/bin/env python3
"""R01-T08 阶段关卡检查器（R01-A15：高风险功能不被演示遮蔽）。

对五个高风险能力域（browser_host / pdf_renderer / shell_capabilities /
storage_cutover / protocol_chain）逐项核验。必需域集合、各域必需能力 ID 与
证据路径、各域递延项 ID 及其风险登记绑定，全部由本文件内的冻结契约
FROZEN_CONTRACT 定义——独立于被验输入（R01 阶段验收 R1 F01 修复：不得以输入
自报的 required_capabilities 为唯一权威；删域/删必需能力/删 sha256/删递延/
改名/重复/额外项一律拒绝）。修改契约 = 修改关卡本身，须经独立验收并在
git 历史中可审计。

  G0 契约闭合：被验输入的域集合、各域必需能力 ID 集合、各域递延项 ID 集合
     必须与冻结契约精确一致——缺项/额外/重复/改名 → NO-GO（BLOCKED）。
  G1 必需能力必须 status=VERIFIED（实施平台实测），证据路径必填且与契约钉住
     路径一致、证据文件存在、sha256 必填、为 64 位小写十六进制且与文件实算
     一致——「截图通过但用户接管失败」这类输入使 browser_host 域判
     NOT_COMPLETE，绝不标完整通过。
  G2 任何 FAILED/UNVERIFIED 的必需能力 → 该域 NOT_COMPLETE。
  G3 证据缺失、证据路径与契约不符、sha256 缺失/非法/不符 → 该域 BLOCKED
     （证据不可信，不猜测）。
  G4 每个递延项 status 必须是合法递延态（UNVERIFIED/OPEN_FINDING/
     REGISTERED_DEFECT/DOCUMENTED），risk_id 必须与冻结契约钉住的绑定一致，
     且 RISK_REGISTER.json 对应条目存在并带 resolve_by_stage 与
     failure_handling；缺失/不符 → BLOCKED（未跟踪的未验证项不得放行）。
  G5（R01 阶段验收 R2 F01 修复）风险登记本体完整性：risks 必须是列表，
     每条是带合法 id 的对象，id 不得重复；递延绑定条目的
     resolve_by_stage / failure_handling 必须是有意义字符串——JSON null、
     对象、数组、数字、布尔、空白串、占位值（tbd/null/none 等）一律拒绝，
     不得以 str() 转换非字符串值冒充（str(None)="None" 这类假非空）；
     递延项输入状态与登记条目状态必须一致（UNVERIFIED/OPEN_FINDING↔
     OPEN/CARRIED，REGISTERED_DEFECT↔REGISTERED，DOCUMENTED↔DOCUMENTED）——
     已收口（CLOSED*）或不匹配的登记不得支撑"仍待后续阶段"的递延挂账。
  G6（R01 阶段验收 R3 F01 修复）截止阶段可机器校验绑定：递延绑定风险条目
     必须带结构化 resolve_by_stage_id / resolve_latest_stage_id（真实字符串
     阶段 ID），并与冻结契约钉住的 deadline_stage / latest_stage、真实阶段
     索引（REAL_STAGE_INDEX，冻结自任务书 stage-index.json 的 R00–R11）、
     resolve_by_stage 正文三方一致——不存在的阶段（如 R99）、无阶段坐标的
     含糊措辞（如"以后"）、不晚于当前阶段 R01 的坐标、与契约归属不符的
     阶段、越过契约最迟关卡的坐标、latest < deadline 的倒挂、正文首个阶段
     token 与结构化字段不符、正文含任何不存在阶段 token，一律 BLOCKED。
     截止期不得只靠正文出现阶段字样冒充：结构化坐标才是判定权威，正文仅
     做一致性核验。
  G6b（R01 阶段验收 R4 F01 修复；R01 阶段修复 R5 扩展；R01 阶段修复 R6 全角折叠）
     阶段引用按完整标识识别、按边界校验：旧 `R\\d{2}` 无边界检查，把 "R099"/"R0999" 截成 R09、
     "XR09"/"R09X"/"R09_stage" 截出 R09，使伪装标识冒充真实截止阶段放行。
     现提取完整形态引用后逐个核验存在于真实阶段索引——"R099" 提取为 "R099"
     而非 "R09"，不存在即拒；凡形似阶段引用（R+数字）但紧邻「标识延续字符」
     的伪装出现单独拒绝（risk-stage-disguised），不得截断成合法阶段放行；
     「最迟 X」标记同样按完整形态捕获并核验。
     合法阶段引用完整语法（R5 明确化，R6 补全角折叠）：引用 = `R` + 至少一位 ASCII 数字组成
     的完整 token，且紧邻前后字符均不属「标识延续字符」——字母/数字/下划线
     （构成更长标识，如 R0999/XR09/R09X/R09_stage）与点号/连字符族连接符
     （构成复合/小阶段/区间/拼接标识，如 R09.5/R09-5/R09．5/R09－5/R09.5.1/
     R09.R10 及全角点、全角下划线、en/em dash、间隔号等变体）。空白、斜杠、
     中文标点（（）／，。、：等）是合法分隔符，分隔两个独立完整引用。结构化
     坐标（resolve_by_stage_id/resolve_latest_stage_id）始终是判定权威，正文
     仅做一致性核验；正文任何形似阶段引用的出现都必须是真实存在的完整引用。
  G6c（R01 阶段验收 R6 F01 修复）全角同形与大小写形态失败关闭：R6 评审实测正文
     首位 `Ｒ９９`（全角 R+全角数字，不存在的截止阶段）或「最迟 Ｒ１０」（全角虚构
     放宽关卡）与合法 ASCII `R09` 混用时完全绕过提取/存在性/首坐标/最迟核验
     （exit 0 / PASS_WITH_CONDITIONS），使人工交接截止与机器判定相矛盾。现正文在
     提取前先按 1:1 等长折叠全角拉丁字母（Ａ-Ｚ/ａ-ｚ）与全角数字（０-９）为
     ASCII——全角同形阶段（Ｒ９９/Ｒ１０/Ｒ０９、混合宽度 Ｒ99/R９９、全角前缀
     复合 ＸＲ０９）折叠后与 ASCII 形态走同一套真实阶段索引/正文首位/最迟一致性
     核验（Ｒ９９→R99 不存在即拒；最迟 Ｒ１０→R10 与结构化最迟 R09 矛盾即拒；
     Ｒ０９→R09 与结构化坐标一致时按规范化引用通过，首坐标校验不再失真）；折叠
     仅用于识别与核验，结构化坐标（resolve_by_stage_id/resolve_latest_stage_id）
     不折叠、保持 ASCII 精确权威，伪装形态报告仍展示原文。大小写契约：合法阶段
     引用仅认大写 R——小写 r+数字（r99、全角小写 ｒ９９ 折叠后同）不是合法引用，
     由伪装检出按完整形态拒绝（risk-stage-disguised），不得在合法引用旁隐身。
  G6d（R01 阶段验收 R7 F01 修复；R01 阶段修复 R8 连接形态扩展）正文每一处显式
  期限标记逐一核验、失败关闭：R6 及以前对折叠后的正文用 LATEST_MARKER_RE.search()
  只校验第一处「最迟」——正文写出两处互相矛盾的强制期限（如「最迟 R09；最迟
  R10」，结构化最迟 R09）时第二处被完全忽略（R7 评审真实 CLI 实测 exit 0 /
  PASS_WITH_CONDITIONS），风险交接正文与机器接受的最迟期限不一致。R7 改为
  finditer 逐处核验：任一标记引用不存在的阶段 → risk-stage-unknown；任一标记与
  结构化 resolve_latest_stage_id 不符 → risk-stage-text-mismatch。R8 发现 R7 的
  标记语法只识别「最迟」后接空白再紧跟阶段 ID：「最迟：R10」「最迟: R10」「最迟于
  R10」「最迟为 R10」及同族否定连接式「不迟于 R10」「不晚于 R10」「不得迟于
  R10」「不得晚于 R10」是同样明确的期限声明，却完全不构成标记（R8 评审真实
  CLI 实测 exit 0 误放，通用阶段扫描只查 R10 存在性、不与结构化最迟比较）。
  现标记 = 触发词族（最迟/不迟于/不得迟于/不晚于/不得晚于）+ 有界连接段（≤16
  字符的空白/冒号/连接词等自然书写杂讯；段内不得出现 R/r 与子句终结符——期限
  声明必须落在同一子句内）+ 完整阶段 ID，逐处核验语义不变。失败关闭：任一触发词
  出现而在其子句内解析不出阶段 ID（如「最迟于第三阶段完成」的自然语言期限）→
  risk-stage-latest-unresolved——无法机器核验的期限不得替代结构化坐标放行。
  清晰契约：正文可写多处显式期限标记，但每一处都是强制期限声明，必须全部与
  结构化最迟关卡一致方可放行（如「最迟 R09；最迟：R09」），任一相矛盾即拒；
  与触发词语法无关的正文正常提及真实阶段（如「R10（后续平台事项）」）不构成
  期限标记，不因本规则被拒。结构化坐标（resolve_latest_stage_id）始终是判定
  权威，正文核验不把权威期限放宽到更晚阶段。
  G6e（R01 阶段验收 R9 F01 修复）单个期限声明子句内唯一一致阶段核验：R8 的标记
  语法把触发词绑定到其后首个阶段 ID，同一子句内首个阶段之后的第二阶段——改期
  （「（原定 R09，现改 R10）完成」）、顺延（「最迟由原定的 R09 顺延至 R10 完成」）、
  选择（「最迟在 R09 或 R10 完成」）与斜杠并列（「最迟 R09/R10 完成」及全角变体）
  ——被通用阶段扫描当作普通上下文只查存在性，使明确改晚的强制期限仍 exit 0 误放
  （R9 评审真实 CLI 实测）。现每个触发词治理「从触发词起到子句终结符为止」的
  完整区域：区域内每个完整阶段引用都必须与结构化 resolve_latest_stage_id 一致，
  出现任何其他阶段即无法机器判定唯一一致期限，按 risk-stage-text-mismatch 失败
  关闭拒绝。不枚举改期/顺延/选择语言（中文「或」、拉丁「or」、半/全角斜杠及其
  组合由「区域内出现第二阶段」统一覆盖，不逐词打补丁）；触发词之前的阶段是截止
  坐标上下文（首坐标校验管辖）、终结符之后的阶段是普通上下文提及（存在性校验
  管辖），均不受本规则约束。触发词被成对引号紧包（“最迟”/「最迟」/『最迟』/
  "最迟"）时为引用词名而非期限声明（如「R10 文档介绍“最迟”字段」），不触发
  解析义务——R10 起该豁免不再覆盖其后续区域治理（见 G6f）。结构化坐标仍是
  判定权威，不放宽到更晚阶段。
  G6f（R01 阶段修复 R10 F01）期限区域完整性与已披露边界期限拒绝：R9 的引号
  紧包豁免实际跳过整个触发词（continue 同时移除解析义务与区域治理），触发词后
  的实际赋值/改期（「R09（宿主集成）“最迟”字段：原定 R09，现改 R10 完成」，
  四种支持引号同族）与斜杠/全角斜杠非唯一期限（「“最迟” R09/R10 完成」、
  「「最迟」 Ｒ０９／Ｒ１０ 完成」）全部逃逸核验（R10 评审真实 CLI 实测 exit 0
  误放）；区域终结符扫描不考虑括号层次，括号内部的分号或换行（「最迟（原定
  R09；现改 R10）完成」）把完整期限声明截断成只含首阶段的区域。现：(1) 引号
  紧包只豁免该触发词自身的解析义务（词名引用无须解析出阶段 ID），其「触发词
  起到括号感知终结符止」的后续区域仍逐引用与结构化 resolve_latest_stage_id
  一致——词名/字段名引用不得遮蔽同一子句内随后的实际赋值/改期/选择（紧包形态
  内部不可能携带阶段 ID 的事实只证明引号内无阶段，不证明其后无期限书写）；
  (2) 区域边界括号感知：处于未闭合全/半角括号内的终结符不终止区域，未闭合括号
  使区域延伸至文本末尾（失败关闭方向——区域只增不减）；(3) 「最晚」并入显式
  期限触发词族（迟/晚对称补全，封闭形态族非开放同义词枚举），「最晚于 R10
  完成」按标记语法逐处核验——R10 评审独立裁定：已披露的明确截止不得凭「文档化
  边界」自动当普通上下文；(4) 以阶段为锚的「前」边界期限书写（「R10 前必须
  完成」「R10 之前完成」——封闭词素族 之前/以前/前 紧邻完整阶段引用）机器契约
  不支持与结构化坐标的一致性核验，按 risk-stage-boundary-unsupported 失败关闭
  拒绝。机器书写/交接契约：期限表达一律用触发词族（最迟/最晚/不迟于/不得迟于/
  不晚于/不得晚于）+ 完整阶段 ID（或结构化坐标），四种支持引号（“”/「」/『』/
  ASCII "）仅用于词名引用且紧包内容不得携带期限；其他自然语言期限形态机器不
  支持识别、也不得据此放宽结构化坐标权威——凡机器已识别为明确期限的书写（含
  上述已披露边界形态）均须可核验一致或被拒绝，不得静默当上下文放行。

总体判定：
  所有域 COMPLETE 且无递延项 → PASS
  所有域 COMPLETE 但有递延项（均已挂账） → PASS_WITH_CONDITIONS
  任一域 NOT_COMPLETE/BLOCKED 或任何契约闭合违例 → NO-GO，明确阻塞替壳路径
  （shell replacement）

用法：
  python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py [--out REPORT.json]
  python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test
  python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --inputs OTHER.json --register OTHER.json
  bash    docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh
      （R2/R3/R4/R5/R6 F01：真实 CLI 负向电池——null/对象/数组/空白/占位/状态矛盾/
      重复 ID 等基础 15 变体 + R3 截止阶段绑定 7 变体（不存在阶段 R99/含糊
      措辞/过晚截止期/越过最迟关卡/缺结构化坐标/坐标倒挂）+ R4 完整标识边界
      6 变体（R099/混合 R09/R099/前缀 XR09/后缀 R09X/最迟 R099）+ R5 复合/
      拼接形态 8 变体（R09.5/R09-5/R09．5/R09－5/R09.5.1/R09.R10/最迟 R09.5/
      混合 R09/R09.5）+ R6 全角同形/大小写形态 7 变体（正文首位 Ｒ９９/最迟
      Ｒ１０/正文首位 Ｒ１０/混合宽度 Ｒ99/全角复合 ＸＲ０９/小写 r99/全角小写
      ｒ９９）+ R7 重复「最迟」逐处核验 3 变体（首处一致/后处冲突「最迟 R09；最迟
      R10」、顺序交换「最迟 R10；最迟 R09」、全角混用「最迟 Ｒ０９；最迟 Ｒ１０」——
      相矛盾的第二处强制期限不得被忽略）+ R8 显式期限连接形态 8 变体（全角冒号
      「最迟：R10」/全角冒号+全角阶段「最迟：Ｒ１０」/连接词「最迟于 R10」/单处
      冒号/半角冒号「最迟: R10」/连接词族「最迟为 R10」/否定连接式「不晚于 R10」/
      触发词无阶段 ID「最迟于第三阶段完成」→ risk-stage-latest-unresolved 失败
      关闭）+ R9 单期限子句第二阶段 8 变体（改期「最迟（原定 R09，现改 R10）完成」/
      顺延「最迟由原定的 R09 顺延至 R10 完成」/选择「最迟在 R09 或 R10 完成」/
      斜杠并列「最迟 R09/R10 完成」/全角改期/全角斜杠「最迟 Ｒ０９／Ｒ１０」/
      全角选择「最迟在 Ｒ０９ 或 Ｒ１０」/拉丁连接「最迟在 R09 or R10」→ 同一
      期限子句内出现与结构化最迟不一致的第二阶段即拒）必须全部 exit 1；
      R10 追加期限区域完整性 12 变体（引号词名后实际改期「“最迟”原定 R09，
      现改 R10 完成」/「“最迟”字段：原定 R09，现改 R10 完成」及「」『』ASCII "
      三种引号同形/引号后斜杠「“最迟” R09/R10 完成」/引号后全角斜杠「「最迟」
      Ｒ０９／Ｒ１０ 完成」/括号内分号「最迟（原定 R09；现改 R10）完成」/括号内
      换行同形/同义词触发词「最晚于 R10 完成」/前边界期限「R10 前必须完成」与
      「R10 之前完成」→ 词名豁免不覆盖后续区域、区域边界括号感知、「最晚」入族
      逐处核验、前边界失败关闭，全部 exit 1）；正向对照
      exit 0，另有全角 Ｒ０９ 规范化一致正向、
      多处「最迟 R09」一致正向、正常上下文提及 R10 不误伤正向、连接形态一致
      （「最迟：R09；最迟于 R09」）与否定连接式一致（「不晚于 R09」）正向、
      同子句内重复一致「最迟 R09 完成（R09 复核）」与引号词名引用
      「R10 文档介绍“最迟”字段」正向、直角引号词名「R10 文档介绍「最迟」字段」
      正向、同义词一致「最晚于 R09 完成」与词名后一致提及「“最迟”字段即 R09」
      正向均 exit 0（11 正+74 负）；--self-test 为
      进程内负向，两者互补，不得以一方代替另一方）

退出码：0 = PASS/PASS_WITH_CONDITIONS；1 = NO-GO；self-test 0 = 全部负向按预期拒绝。
"""
import argparse
import copy
import hashlib
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DEFAULT_INPUTS = REPO / "docs/rust-tauri/R01/r01_t08_gate_inputs.json"
DEFAULT_REGISTER = REPO / "docs/rust-tauri/R01/RISK_REGISTER.json"
DEFERRED_STATUSES = {"UNVERIFIED", "OPEN_FINDING", "REGISTERED_DEFECT", "DOCUMENTED"}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")

CONTRACT_VERSION = "1.10-stage-repair-r10"

# 真实阶段索引（R01 阶段验收 R3 F01）：任务书 Lingxi_Rust_Tauri_Taskbooks_2026-09-23
# stage-index.json 冻结了 12 个阶段 R00–R11，顺序即执行序。该任务书目录未入 git
# 跟踪，检查器据此在自身内冻结同一份索引，保证关卡在任何检出上都可复现、可审计；
# 修改本列表 = 修改关卡本身，须经独立验收。递延项的截止阶段必须严格晚于当前
# 阶段 R01 且真实存在，最迟关卡不得越过冻结契约钉住的上界。
REAL_STAGE_INDEX = ["R00", "R01", "R02", "R03", "R04", "R05", "R06", "R07", "R08", "R09", "R10", "R11"]
STAGE_ORDER = {stage: i for i, stage in enumerate(REAL_STAGE_INDEX)}
CURRENT_STAGE = "R01"
# R01 阶段验收 R4 F01（R01 阶段修复 R5 扩展）：合法阶段引用 = R+1 位以上数字的
# 完整 token，且紧邻前后字符均不属「标识延续字符」。延续字符 = 字母/数字/下划线
# （更长标识：R0999/XR09/R09X/R09_stage）+ 点号/连字符族连接符（复合/小阶段/
# 区间/拼接：R09.5/R09-5/R09．5/R09－5/R09.5.1/R09.R10，含全角点/全角下划线/
# 全角连字符/en–em dash/间隔号等变体）——这些字符紧贴 token 表示整个出现属于
# 一个更长的复合标识，不是完整合法引用。空白、斜杠 /、中文标点（（）／，。、：
# 等）是合法分隔符，分隔两个独立完整引用；正文中不出现任何形似引用（R+数字）
# 时由 vague 规则拒绝含糊措辞。结构化坐标始终是判定权威。
STAGE_EDGE_CHARS = "A-Za-z0-9_.\\-．－＿﹒–—―‐‑‒−·・"
_STAGE_EDGE_CLASS = f"[{STAGE_EDGE_CHARS}]"
STAGE_TOKEN_RE = re.compile(rf"(?<!{_STAGE_EDGE_CLASS})R\d+(?!{_STAGE_EDGE_CLASS})")
# R01 阶段验收 R6 F01：伪装检出扫描在「全角折叠后」的正文上进行（见
# _fold_stage_homoglyphs），并带 IGNORECASE——大小写契约为「合法阶段引用仅认
# 大写 R」，小写 r+数字（含全角小写 ｒ９９ 折叠后形态）不是合法引用，但必须被
# 识别为形似出现的伪装形态单独拒绝，不得在合法引用旁隐身（R6 评审实测 r99 与
# 合法 R09 混用 exit 0 误放）。
STAGE_DISGUISE_RE = re.compile(r"R\d+", re.IGNORECASE)
# R01 阶段修复 R8 F01（G6d 连接形态扩展）：显式期限触发词族——「最迟」及其同义
# 否定连接式「不迟于/不得迟于/不晚于/不得晚于」。R7 及以前 LATEST_MARKER_RE 只
# 识别「最迟」后接空白再紧跟阶段 ID 的形态：「最迟：R10」「最迟: R10」「最迟于
# R10」「最迟为 R10」等常见自然书写（全角/半角冒号、连接词）与同族否定连接式是
# 同样明确的期限声明，却完全不构成标记（R8 评审真实 CLI 实测：结构化最迟 R09
# 时正文写出放宽到 R10 的第二处期限被 exit 0 误放，绕过 R7 逐处核验）。
# R01 阶段修复 R10 F01（G6f）：「最晚」并入触发词族——迟/晚对称补全（既有族已含
# 不迟于/不得迟于/不晚于/不得晚于的迟/晚两系，唯独缺「最晚」），系封闭形态族
# 补全而非开放同义词枚举。R10 评审独立裁定：「最晚于 R10 完成」这类已披露的
# 明确截止不得凭「文档化边界」自动当普通上下文——入族后按标记语法逐处核验，
# 与结构化最迟不一致即拒（一致书写「最晚于 R09 完成」照常放行）。
LATEST_TRIGGER_RE = re.compile(r"不得迟于|不得晚于|不迟于|不晚于|最迟|最晚")
# 标记 = 触发词族 + 有界连接段 + 完整阶段 ID（折叠后正文上匹配）。连接段是
# 触发词与阶段 ID 之间的自然书写杂讯——空白/全半角冒号/连接词/其他非阶段起点
# 字符，最长 16 字符：期限声明必须落在同一子句内，跨子句的阶段是上下文提及，
# 不是该触发词的期限。段内不得出现大写 R 或小写 r（可能的阶段/伪装起点——
# 出现即视为该触发词在本子句内无法解析出阶段，不得静默跳过），也不得跨越
# 子句/句子终结符（；;。．.！!？?换行）。
LATEST_MARKER_RE = re.compile(
    rf"(?:不得迟于|不得晚于|不迟于|不晚于|最迟|最晚)"
    rf"[^Rr；;。．.！!？?\n\r]{{0,16}}"
    rf"(?<!{_STAGE_EDGE_CLASS})(R\d+(?!{_STAGE_EDGE_CLASS}))"
)
# R01 阶段修复 R9 F01（G6e 期限子句唯一一致）：显式期限触发词的治理区域 = 从
# 触发词起到子句终结符为止（与 G6d 连接段的终结符集同一字符族）。区域内每个
# 完整阶段引用都必须与结构化 resolve_latest_stage_id 一致——同一期限声明子句内
# 出现第二个阶段（改期「原定 R09，现改 R10」/顺延「顺延至 R10」/选择「或 R10」/
# 斜杠并列「R09/R10」及全角变体）即无法机器判定唯一一致期限，失败关闭拒绝；
# 不枚举改期/顺延/选择语言，任何第二阶段统一覆盖。
CLAUSE_TERMINATOR_CHARS = "；;。．.！!？?\n\r"
# R01 阶段修复 R10 F01（G6f 区域边界括号感知）：R9 的区域终结符扫描不考虑括号
# 层次——括号内部的分号/换行把「最迟（原定 R09；现改 R10）完成」这类完整括号
# 期限声明截断成只含首阶段的区域（R10 评审真实 CLI 实测 exit 0 误放）。现区域
# 边界扫描维护全/半角括号深度：处于未闭合括号内的终结符不终止区域；触发词之前
# 未闭合的括号计入初始深度；未闭合括号使区域延伸至文本末尾（失败关闭方向——
# 区域只增不减，多核验的引用只会多拒不会少拒）。首坐标校验、存在性校验与标记
# 连接段约束不变（标记连接段仍不得跨任何终结符——「最迟（说明；R10）」这类
# 首阶段被终结符隔开的书写按 risk-stage-latest-unresolved 失败关闭）。
PAREN_OPEN_CHARS = "（("
PAREN_CLOSE_CHARS = "）)"
# R01 阶段修复 R9 F01（G6e 引用词名豁免；R10 F01 收窄）：触发词被成对引号紧包
# （“最迟”/「最迟」/『最迟』/"最迟"）时是引用词名/字段名，不是期限声明。R9 的
# 豁免跳过整个触发词（解析义务+区域治理），R10 评审实测其遮蔽了词名之后的实际
# 赋值/改期/选择（「“最迟”字段：原定 R09，现改 R10 完成」exit 0 误放）；R10 起
# 收窄为只豁免该触发词自身的解析义务（词名引用无须解析出阶段 ID，R9 评审披露
# 「R10 文档介绍“最迟”字段」误拒限制由此保持），其后续区域仍按 G6e/G6f 治理
# ——词名引用不得作为其后续不一致阶段的遮蔽。
TRIGGER_QUOTE_PAIRS = {("“", "”"), ("「", "」"), ("『", "』"), ('"', '"')}
# R01 阶段修复 R10 F01（G6f 已披露「前」边界期限拒绝）：「R10 前必须完成」这类
# 以阶段为锚的「前」边界书写是明确的期限声明，但其「before-stage」语义无法与
# 结构化坐标（resolve_by_stage_id/resolve_latest_stage_id，by-stage 语义）机器
# 判定一致；R10 评审独立裁定其不得凭「文档化边界」自动当普通上下文。检测 =
# 完整阶段引用（完整标识边界、全角折叠后）紧邻（仅空白）「之前/以前/前」——
# 封闭词素族，不开放枚举；命中即按 risk-stage-boundary-unsupported 失败关闭
# 拒绝（该形态无一致写法，表达期限请改用触发词族并与结构化坐标一致）。
PRE_BOUNDARY_RE = re.compile(
    rf"(?<!{_STAGE_EDGE_CLASS})(R\d+)(?!{_STAGE_EDGE_CLASS})\s*(?:之前|以前|前)"
)
# 复合形态展开用的单字符判定集（与 STAGE_EDGE_CHARS 同一字符族；集合需逐字符
# 枚举，不能用正则区间写法）。
STAGE_EDGE_SET = (set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_")
                  | set(".-．－＿﹒–—―‐‑‒−·・"))


def _fold_stage_homoglyphs(text):
    """（R01 阶段验收 R6 F01，G6c）1:1 等长折叠全角同形字母/数字为 ASCII。

    全角拉丁字母 Ａ-Ｚ（U+FF21–FF3A）、ａ-ｚ（U+FF41–FF5A）与全角数字
    ０-９（U+FF10–FF19）逐字符映射为对应 ASCII。映射严格 1:1 等长——折叠后
    的正文与原文逐偏移对齐，识别/核验在折叠文本上进行（全角同形阶段与 ASCII
    形态走同一套真实阶段索引/正文首位/最迟一致性核验），而伪装形态的报告切片
    仍取原文，保证诊断如实展示登记内容。其余字符（含全角点/连字符/下划线等
    R5 边界族、中文标点、空白）不折叠：它们已由标识延续字符类/分隔符规则覆盖。
    本折叠只用于正文一致性核验；结构化坐标（resolve_by_stage_id 等）不折叠，
    非 ASCII 精确形态的结构化坐标仍按不存在于阶段索引拒绝（坐标权威不放宽）。
    """
    out = []
    for ch in text:
        o = ord(ch)
        if 0xFF10 <= o <= 0xFF19:        # ０-９ → 0-9
            out.append(chr(o - 0xFF10 + 0x30))
        elif 0xFF21 <= o <= 0xFF3A:      # Ａ-Ｚ → A-Z
            out.append(chr(o - 0xFF21 + 0x41))
        elif 0xFF41 <= o <= 0xFF5A:      # ａ-ｚ → a-z
            out.append(chr(o - 0xFF41 + 0x61))
        else:
            out.append(ch)
    return "".join(out)

# 递延输入状态 → 风险登记条目允许的状态（R01 阶段验收 R2 F01：状态一致性）。
# 登记侧已收口状态（CLOSED_IN_R01 及任何未列出/缺失状态）都与"该能力仍未
# 验证、挂账待后续阶段"矛盾，不得支撑递延放行。
DEFERRED_RISK_STATUS_MAP = {
    "UNVERIFIED": {"OPEN", "CARRIED"},
    "OPEN_FINDING": {"OPEN", "CARRIED"},
    "REGISTERED_DEFECT": {"REGISTERED"},
    "DOCUMENTED": {"DOCUMENTED"},
}

# 占位/无信息字段值（小写比较）：语义上等于"没有值"，不算有意义字符串。
PLACEHOLDER_FIELD_VALUES = {
    "", "null", "none", "n/a", "na", "tbd", "todo", "unknown", "{}", "[]", "-", "--", "?",
}


def _meaningful_str(v):
    """有意义字符串：JSON 字符串类型、去空白后非空、且不是占位值。

    不得以 str() 转换 None/对象/数组/数字冒充非空（str(None)="None"、
    str({})="{}" 在旧实现里被误判为有效——R01 阶段验收 R2 F01）。
    """
    return isinstance(v, str) and v.strip().lower() not in PLACEHOLDER_FIELD_VALUES


def _extract_stage_references(text):
    """（R01 阶段验收 R4 F01，G6b；R01 阶段修复 R5 扩展；R01 阶段修复 R6 全角折叠）

    按完整标识提取正文阶段引用。提取前先经 _fold_stage_homoglyphs 把全角同形
    字母/数字 1:1 等长折叠为 ASCII（R6 F01：全角同形阶段 Ｒ９９/Ｒ１０/ＸＲ０９
    与混合宽度 Ｒ99/R９９ 曾在合法 ASCII 引用旁完全隐身放行）；折叠不改变偏移，
    伪装形态报告切片仍取原文。

    返回 (clean_refs, disguised_shapes)：
    - clean_refs：(规范化 token, 原文形态) 二元组列表——完整形态引用（R+1 位以上
      数字，紧邻前后字符均非标识延续字符——字母/数字/下划线/点号/连字符族/间隔
      号）。不截断——"R099" 提取为 "R099"，后续按完整串核验存在性；规范化 token
      用于核验（全角 Ｒ０９ 规范化后就是 R09），原文形态用于诊断展示；
    - disguised_shapes：形似阶段引用（R+数字，含小写——大小写契约仅认大写 R）但
      不构成完整引用的出现，向两侧扩展至完整复合形态后按原文报告（如
      "XR09"/"R09X"/"R09_stage"/"R09.5"/"R09-5"/"R09．5"/"R09.5.1"/"R09.R10"/
      "r99"）——单独拒绝，不得截断成合法阶段放行。
    """
    folded = _fold_stage_homoglyphs(text)
    clean_spans = [m.span() for m in STAGE_TOKEN_RE.finditer(folded)]
    clean_refs = [(folded[s:e], text[s:e]) for s, e in clean_spans]

    def compound_shape(s, e):
        while s > 0 and folded[s - 1] in STAGE_EDGE_SET:
            s -= 1
        while e < len(folded) and folded[e] in STAGE_EDGE_SET:
            e += 1
        return s, e

    shapes = []
    for m in STAGE_DISGUISE_RE.finditer(folded):
        s, e = m.span()
        if any(cs <= s and e <= ce for cs, ce in clean_spans):
            continue  # 完整引用的子串，已被完整识别（如 "R099" 内的 "R09"）
        s2, e2 = compound_shape(s, e)
        shapes.append(text[s2:e2])
    return clean_refs, list(dict.fromkeys(shapes))


def _stage_binding_problems(text, stage_id_raw, latest_id_raw, deadline_stage, latest_stage):
    """截止阶段绑定核验（R3 F01 G6 + R4 F01 G6b + R5 边界扩展 + R6 F01 G6c 全角折叠 + R7 F01 G6d 最迟逐处核验 + R8 F01 G6d 连接形态扩展 + R9 F01 G6e 期限子句唯一一致 + R10 F01 G6f 期限区域完整性）。

    返回 (类别, 说明) 列表；为空 = 通过。反例全部拒绝：不存在的阶段（含完整
    形态提取的 R099/R0999 等更长标识、全角同形折叠后的 R99/R10 等）、伪装/截断/
    复合形态（XR09/R09X/R09.5/R09-5/R09．5/ＸＲ０９/r99 等紧邻标识延续字符或不符
    大小写契约的出现，按完整复合形态报告）、含糊无坐标、过晚/错误归属（与冻结
    契约不符）、正文与结构化坐标不一致、正文任一处显式期限标记（最迟/最晚/
    不迟于/不得迟于/不晚于/不得晚于 + 冒号/连接词等自然书写连接段 + 阶段 ID）与结构化
    最迟关卡不符或引用不存在阶段（G6d——每一处显式期限声明都是强制期限，逐处
    核验，多处全部一致方可放行，任一相矛盾即拒）、显式期限触发词在子句内解析
    不出阶段 ID（G6d R8——自然语言期限无法机器核验，失败关闭拒）、单个期限声明
    子句（触发词起到括号感知终结符止）内出现与结构化最迟不一致的第二阶段
    （G6e R9——改期/顺延/选择「或」/斜杠并列及全角变体使唯一一致期限无法机器
    判定，失败关闭拒；被成对引号紧包的触发词是引用词名，豁免其自身解析义务，
    但 R10 起不豁免其后续区域治理——词名引用不得遮蔽同子句内随后的实际赋值/
    改期/选择）、括号内的分号/换行不再截断完整期限声明（G6f R10——区域边界
    括号感知，未闭合括号延伸区域至末尾）、以阶段为锚的「之前/以前/前」边界
    期限书写（G6f R10——机器契约不支持 before-stage 语义与结构化坐标的一致性
    核验，按 risk-stage-boundary-unsupported 失败关闭拒）。
    正文提取前先按 1:1 折叠全角同形字母/数字（G6c）；结构化坐标不折叠，保持
    ASCII 精确权威。
    text 须已通过 _meaningful_str（调用方保证）。
    """
    problems = []

    def field_problem(field, value, label):
        if value is None or (isinstance(value, str) and not value.strip()):
            problems.append((f"risk-stage-field-missing",
                             f"{label}（{field}）缺失——截止阶段必须有结构化坐标"))
        elif not isinstance(value, str):
            problems.append((f"risk-stage-field-missing",
                             f"{label}（{field}）非字符串（{type(value).__name__}）——拒绝 str() 冒充"))

    field_problem("resolve_by_stage_id", stage_id_raw, "截止阶段 ID")
    field_problem("resolve_latest_stage_id", latest_id_raw, "最迟关卡 ID")
    if problems:
        return problems

    stage_id, latest_id = stage_id_raw.strip(), latest_id_raw.strip()
    refs, disguised = _extract_stage_references(text)
    tokens = [norm for norm, _raw in refs]
    raw_of = {}
    for norm, raw in refs:
        raw_of.setdefault(norm, raw)

    def shown(tok):
        raw = raw_of.get(tok, tok)
        return raw if raw == tok else f"{raw}（规范化 {tok}）"

    if not tokens and not disguised:
        problems.append(("risk-stage-vague",
                         f"resolve_by_stage 正文无任何阶段坐标（含糊措辞不得作为截止期）: {text.strip()[:40]!r}"))
    else:
        for ref in disguised:
            problems.append(("risk-stage-disguised",
                             f"resolve_by_stage 正文含伪装/截断/复合形态的阶段引用 {ref!r}"
                             f"（紧邻字母/数字/下划线或点号/连字符族连接符构成更长、虚构小阶段"
                             f"或拼接标识，或为不符大小写契约的小写形态）——完整标识核验下"
                             f"不得截断成合法阶段放行，也不得在合法引用旁隐身"))
        for tok in dict.fromkeys(tokens):
            if tok not in STAGE_ORDER:
                problems.append(("risk-stage-unknown",
                                 f"resolve_by_stage 正文引用的阶段 {shown(tok)} 不存在于真实阶段索引"
                                 f"（R00–R11）——不存在的阶段不得挂账"))
        if tokens and tokens[0] != stage_id:
            problems.append(("risk-stage-text-mismatch",
                             f"resolve_by_stage 正文首个阶段坐标 {shown(tokens[0])} 与结构化截止阶段"
                             f" {stage_id} 不一致——不得以正文措辞绕开结构化绑定"))
        # R8 F01（G6d 连接形态扩展）：标记 = 触发词族（最迟/不迟于/不得迟于/
        # 不晚于/不得晚于）+ 有界连接段（冒号/连接词等自然书写）+ 阶段 ID——
        # R7 语法只认「最迟」后接空白，「最迟：R10」「最迟于 R10」等明确期限被
        # 完全漏检（R8 评审实测 exit 0 误放）。每个触发词出现都必须解析出阶段
        # ID，否则按 risk-stage-latest-unresolved 失败关闭（自然语言期限无法
        # 机器核验一致性，不得放行）。
        # R9 F01（G6e 期限子句唯一一致）：R8 标记语法只把触发词绑定到其后首个
        # 阶段 ID——同一子句内首个阶段之后的第二阶段（改期「（原定 R09，现改
        # R10）完成」/顺延「由原定的 R09 顺延至 R10」/选择「在 R09 或 R10」/
        # 斜杠并列「R09/R10」及全角变体）被通用阶段扫描当普通上下文只查存在性，
        # 明确改晚的强制期限仍 exit 0 误放（R9 评审真实 CLI 实测）。现每个
        # 触发词治理「从触发词起到（括号感知）子句终结符止」的完整区域：区域
        # 内每个完整阶段引用都必须与结构化 latest 一致，出现任何其他阶段即无法
        # 机器判定唯一一致期限，失败关闭拒绝——不枚举改期/顺延/选择语言，任何
        # 第二阶段统一覆盖。
        # R10 F01（G6f 期限区域完整性）：R9 的引号紧包豁免 continue 跳过整个
        # 触发词，实际移除了完整后续区域的核验义务——「被引号包裹的几个字内
        # 无法携带阶段」只证明引号内部没有阶段，不能证明引号后没有对该字段
        # 赋期限；区域终结符也不看括号层次，括号内分号/换行截断完整改期声明。
        # 现引号紧包只豁免解析义务（词名引用无须解析出阶段 ID），区域治理对
        # 全部触发词（含词名引用形态）生效；区域边界括号感知；「最晚」入族；
        # 阶段锚定「前」边界期限按 risk-stage-boundary-unsupported 失败关闭。
        folded = _fold_stage_homoglyphs(text)
        markers = list(LATEST_MARKER_RE.finditer(folded))
        for marker_idx, marker in enumerate(markers, 1):
            mk = marker.group(1)
            mk_shown = text[marker.start(1):marker.end(1)]
            if mk_shown != mk:
                mk_shown = f"{mk_shown}（规范化 {mk}）"
            marker_shown = text[marker.start():marker.end()]
            if mk not in STAGE_ORDER:
                problems.append(("risk-stage-unknown",
                                 f"resolve_by_stage 正文第 {marker_idx} 处显式期限标记「{marker_shown}」引用的阶段"
                                 f"{mk_shown} 不存在于真实阶段索引（R00–R11）——伪装/不存在的最迟关卡不得挂账"))
            elif mk != latest_id:
                problems.append(("risk-stage-text-mismatch",
                                 f"resolve_by_stage 正文第 {marker_idx} 处显式期限标记「{marker_shown}」与结构化最迟关卡"
                                 f" {latest_id} 不一致——正文每一处显式期限声明均为强制期限，"
                                 f"须逐一与结构化坐标一致，相矛盾的期限不得被忽略"))
        marker_spans = [m.span() for m in markers]
        region_refs = list(STAGE_TOKEN_RE.finditer(folded))

        def _quoted_trigger_word(start, end):
            # 触发词被成对引号紧包（“最迟”/「最迟」/『最迟』/"最迟"）→ 引用
            # 词名/字段名，非期限声明，豁免其自身解析义务（R9 评审披露
            # 「R10 文档介绍“最迟”字段」误拒限制的处理保持）。R10 起不再据此
            # 跳过区域治理——词名引用不能遮蔽其后同子句内的实际期限书写。
            if start == 0 or end >= len(folded):
                return False
            return (folded[start - 1], folded[end]) in TRIGGER_QUOTE_PAIRS

        def _region_clause_end(start):
            # 区域边界括号感知（R10 F01 G6f）：从 start 起扫描首个处于括号深度
            # 0 的终结符；start 之前未闭合的括号计入初始深度（触发词可位于括号
            # 注记内）；未闭合括号使区域延伸至文本末尾（失败关闭方向）。
            depth = sum(folded.count(ch, 0, start) for ch in PAREN_OPEN_CHARS) \
                - sum(folded.count(ch, 0, start) for ch in PAREN_CLOSE_CHARS)
            if depth < 0:
                depth = 0
            for i in range(start, len(folded)):
                ch = folded[i]
                if ch in PAREN_OPEN_CHARS:
                    depth += 1
                elif ch in PAREN_CLOSE_CHARS:
                    if depth > 0:
                        depth -= 1
                elif ch in CLAUSE_TERMINATOR_CHARS and depth == 0:
                    return i
            return len(folded)

        for trig_idx, trig in enumerate(LATEST_TRIGGER_RE.finditer(folded), 1):
            ts, te = trig.span()
            quoted = _quoted_trigger_word(ts, te)
            if not quoted and not any(ms <= ts and te <= me for ms, me in marker_spans):
                problems.append(("risk-stage-latest-unresolved",
                                 f"resolve_by_stage 正文第 {trig_idx} 处显式期限触发词「{trig.group(0)}」"
                                 f"在子句内解析不出阶段 ID（如自然语言期限「最迟于第三阶段完成」）——"
                                 f"无法核验与结构化最迟关卡 {latest_id} 的一致性，失败关闭拒绝"
                                 f"（不得以无法机器核验的期限替代结构化坐标）"))
                continue
            clause_end = _region_clause_end(ts)
            for ref in region_refs:
                rs = ref.start()
                if ts <= rs < clause_end and ref.group() != latest_id:
                    ref_shown = text[rs:ref.end()]
                    if ref_shown != ref.group():
                        ref_shown = f"{ref_shown}（规范化 {ref.group()}）"
                    if quoted:
                        problems.append(("risk-stage-text-mismatch",
                                         f"resolve_by_stage 正文第 {trig_idx} 处被引号紧包的词名引用「{trig.group(0)}」"
                                         f"之后同子句内出现与结构化最迟关卡 {latest_id} 不一致的第二阶段 {ref_shown}"
                                         f"（词名/字段名引用不豁免其随后区域内的实际赋值/改期/选择——"
                                         f"引号只包裹词名，包裹不住其后的期限书写）——"
                                         f"无法确定唯一一致期限即拒绝"))
                    else:
                        problems.append(("risk-stage-text-mismatch",
                                         f"resolve_by_stage 正文第 {trig_idx} 处显式期限触发词「{trig.group(0)}」的期限子句内"
                                         f"出现与结构化最迟关卡 {latest_id} 不一致的第二阶段 {ref_shown}"
                                         f"（改期/顺延/选择「或」/斜杠并列等使唯一一致期限无法机器判定）——"
                                         f"同一期限声明子句内全部阶段引用必须与结构化最迟一致，"
                                         f"无法确定唯一一致期限即拒绝"))

        # R10 F01（G6f 已披露「前」边界期限拒绝）：以阶段为锚的「之前/以前/前」
        # 边界书写（如「R10 前必须完成」）是明确期限形态，机器契约不支持其与
        # 结构化坐标的一致性核验——失败关闭拒绝，不得凭「文档化边界」当普通
        # 上下文放行（R10 评审独立裁定）。封闭词素族检测，无一致写法。
        for m in PRE_BOUNDARY_RE.finditer(folded):
            stage_shown = text[m.start():m.end(1)]
            if stage_shown != m.group(1):
                stage_shown = f"{stage_shown}（规范化 {m.group(1)}）"
            boundary_shown = text[m.start():m.end()]
            problems.append(("risk-stage-boundary-unsupported",
                             f"resolve_by_stage 正文含以阶段 {stage_shown} 为锚的「前」边界期限书写「{boundary_shown}」"
                             f"（如「R10 前必须完成」）——机器契约仅支持触发词族（最迟/最晚/不迟于/不得迟于/"
                             f"不晚于/不得晚于）+ 完整阶段 ID 的期限语法，「前」边界期限无法与结构化截止/最迟"
                             f"坐标机器判定一致，失败关闭拒绝（表达期限请改用触发词族写法并与结构化坐标一致）"))

    if stage_id not in STAGE_ORDER:
        problems.append(("risk-stage-unknown",
                         f"结构化截止阶段 {stage_id} 不存在于真实阶段索引（R00–R11）"))
    if latest_id not in STAGE_ORDER:
        problems.append(("risk-stage-unknown",
                         f"结构化最迟关卡 {latest_id} 不存在于真实阶段索引（R00–R11）"))
    if problems:
        return problems

    if STAGE_ORDER[stage_id] <= STAGE_ORDER[CURRENT_STAGE]:
        problems.append(("risk-stage-current-or-past",
                         f"截止阶段 {stage_id} 不晚于当前阶段 {CURRENT_STAGE}——递延项必须落到后续阶段"))
        return problems
    if STAGE_ORDER[latest_id] < STAGE_ORDER[stage_id]:
        problems.append(("risk-stage-order-inverted",
                         f"最迟关卡 {latest_id} 早于截止阶段 {stage_id}——阶段坐标倒挂"))
        return problems
    if stage_id != deadline_stage:
        problems.append(("risk-stage-deadline-mismatch",
                         f"截止阶段 {stage_id} 与冻结契约钉住的 {deadline_stage} 不符"
                         f"（错误归属或过晚的截止期不得放行）"))
    if latest_id != latest_stage:
        problems.append(("risk-stage-latest-mismatch",
                         f"最迟关卡 {latest_id} 越过冻结契约钉住的 {latest_stage}"
                         f"——不得把递延推到适用最迟关卡之后"))
    return problems

# 冻结契约（R01 阶段验收 R1 F01 修复引入）：独立于被验输入的权威闭合集合。
# 五个必需域；每域必需能力 ID → 契约钉住的证据路径；每域递延项 ID → 契约钉住的
# RISK_REGISTER 条目绑定与截止阶段契约（R01 阶段验收 R3 F01：deadline_stage =
# 该项适用截止阶段，latest_stage = 适用最迟关卡；取值冻结自风险登记各条目
# resolve_by_stage 正文已声明并经 R01 阶段关卡放行的坐标，例如跨平台/授权态
# 缺口的 R09/R10 强制关卡）。被验输入（r01_t08_gate_inputs.json）只是这些事实
# 的声明方，本契约才是判定的权威来源。
FROZEN_CONTRACT = {
    "browser_host": {
        "required": {
            "navigation_snapshot_input_scroll": "artifacts/rust-tauri/R01/T04/chromium/summary.json",
            "viewport_screenshot": "artifacts/rust-tauri/R01/T04/chromium/summary.json",
            "session_isolation": "artifacts/rust-tauri/R01/T04/chromium-iso/summary.json",
            "untrusted_page_sandbox": "artifacts/rust-tauri/R01/T04/chromium-proxy/summary.json",
            "user_takeover": "artifacts/rust-tauri/R01/T04/chromium-takeover/summary.json",
        },
        "deferred": {
            "fetch_interception_layer": {
                "risk_id": "RR-T05-X1", "deadline_stage": "R09", "latest_stage": "R09",
            },
            "webrtc_exfiltration_boundary": {
                "risk_id": "RR-T05-N2", "deadline_stage": "R09", "latest_stage": "R09",
            },
        },
    },
    "pdf_renderer": {
        "required": {
            "chinese_longdoc_fidelity": "artifacts/rust-tauri/R01/T05/a09/compare.json",
            "dangerous_resource_denial": "artifacts/rust-tauri/R01/T05/a10-dangerous/new/run-result.json",
            "infinite_script_timeout": "artifacts/rust-tauri/R01/T05/a10-infinite/new/run-result.json",
            "ws_loopback_boundary": "artifacts/rust-tauri/R01/T05/repair-r1/negative/negative-verdicts.txt",
            "matrix_28_verified": "artifacts/rust-tauri/R01/T05/matrix-verdicts.json",
        },
        "deferred": {},
    },
    "shell_capabilities": {
        "required": {
            "window_tray_shortcut_notification_clipboard_dialog_accessibility_updater_loginstartup_restart": "artifacts/rust-tauri/R01/T06/runner-summary.txt",
            "sidecar_lifecycle_acl": "artifacts/rust-tauri/R01/T06/runner-summary.txt",
            "capability_matrix_source": "docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json",
        },
        "deferred": {
            "recording_authorized_path": {
                "risk_id": "RR-T06-MIC", "deadline_stage": "R09", "latest_stage": "R09",
            },
            "speech_recognition_authorized_chain": {
                "risk_id": "RR-T06-SPEECH", "deadline_stage": "R09", "latest_stage": "R09",
            },
            "screen_real_capture": {
                "risk_id": "RR-T06-SCREEN", "deadline_stage": "R09", "latest_stage": "R09",
            },
            "proxy_matrix": {
                "risk_id": "RR-T06-PROXY", "deadline_stage": "R09", "latest_stage": "R10",
            },
            "cross_platform_windows_linux_macx64": {
                "risk_id": "RR-T06-PLATFORM", "deadline_stage": "R09", "latest_stage": "R10",
            },
        },
    },
    "storage_cutover": {
        "required": {
            "old_binary_write_refusal": "artifacts/rust-tauri/R01/T07/a13/a7b-old-on-active-epoch2-refused.log",
            "rollback_no_data_loss_drill": "artifacts/rust-tauri/R01/T07/a14/rollback-drill-summary.json",
            "data_compatibility_matrix": "docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json",
        },
        "deferred": {
            "corrupt_failure_failopen_fix": {
                "risk_id": "RR-T07-PROD-DEFECT-1", "deadline_stage": "R02", "latest_stage": "R02",
            },
            "shell_local_stores_gate": {
                "risk_id": "RR-T07-F3", "deadline_stage": "R09", "latest_stage": "R09",
            },
            "cooperative_gate_physical_separation": {
                "risk_id": "RR-T07-F2", "deadline_stage": "R08", "latest_stage": "R09",
            },
        },
    },
    "protocol_chain": {
        "required": {
            "cross_language_roundtrip": "artifacts/rust-tauri/R01/T08/gates/t02-roundtrip-fresh-target.log",
            "version_incompatibility_diagnosis": "artifacts/rust-tauri/R01/T08/gates/t02-handshake-fresh-target.log",
            "generated_drift_free": "artifacts/rust-tauri/R01/T08/gates/t02-check-generated-fresh-target.log",
            "ownership_machine_gate": "docs/rust-tauri/R01/OWNERSHIP_TARGET.json",
        },
        "deferred": {
            "canonical_non_bmp_divergence": {
                "risk_id": "RR-T02-F1", "deadline_stage": "R04", "latest_stage": "R04",
            },
            "ts_canonical_float_bigint": {
                "risk_id": "RR-T02-F2", "deadline_stage": "R04", "latest_stage": "R04",
            },
            "eventpayload_fallback_invariants": {
                "risk_id": "RR-T02-F3", "deadline_stage": "R04", "latest_stage": "R04",
            },
            "seq_parse_lenient": {
                "risk_id": "RR-T02-F4", "deadline_stage": "R04", "latest_stage": "R04",
            },
            "ws_close_code_hardcoded": {
                "risk_id": "RR-T02-F5", "deadline_stage": "R08", "latest_stage": "R08",
            },
        },
    },
}


def _load(p):
    with open(p, "r", encoding="utf-8") as f:
        return json.load(f)


def _sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def _index_caps(caps):
    """能力列表 → (按 id 首次出现建索引, 重复 id 列表)。非 dict/缺 id 条目计入问题由调用方判。"""
    by_id = {}
    dups = []
    if not isinstance(caps, list):
        return by_id, ["<non-list>"]
    for cap in caps:
        cid = cap.get("id") if isinstance(cap, dict) else None
        if cid is None:
            dups.append("<missing-id>")
            continue
        if cid in by_id:
            dups.append(cid)
        else:
            by_id[cid] = cap
    return by_id, dups


def evaluate(inputs, register, repo=REPO, contract=FROZEN_CONTRACT):
    """返回 (verdict, report)。verdict ∈ PASS / PASS_WITH_CONDITIONS / NO-GO。"""
    # G5 登记本体完整性（R2 F01）：risks 列表逐条校验——非列表/非对象/缺 id/
    # 重复 id 都使登记不可信，一律 NO-GO；不再静默被后写条目覆盖。
    risk_by_id = {}
    register_violations = []
    risks_raw = register.get("risks") if isinstance(register, dict) else None
    if not isinstance(risks_raw, list):
        register_violations.append("风险登记 risks 缺失或非列表——登记不可信，递延挂账无法核验")
        risks_raw = []
    for pos, entry in enumerate(risks_raw):
        if not isinstance(entry, dict):
            register_violations.append(f"风险登记第 {pos} 条不是对象——登记不可信")
            continue
        rid = entry.get("id")
        if not isinstance(rid, str) or not rid.strip():
            register_violations.append(f"风险登记第 {pos} 条缺合法 id——登记不可信")
            continue
        if rid in risk_by_id:
            register_violations.append(
                f"风险登记 id {rid} 重复登记——登记不可信（重复 ID 不得以后写覆盖先写）")
            continue
        risk_by_id[rid] = entry
    areas_in = inputs.get("areas")
    if not isinstance(areas_in, dict):
        areas_in = {}
    areas_report = {}
    blocking = []
    deferred_total = 0

    expected_areas = set(contract)
    actual_areas = set(areas_in)
    contract_violations = []
    for name in sorted(expected_areas - actual_areas):
        contract_violations.append(f"必需域 {name} 整体缺失——冻结契约闭合失败（删域不得放行）")
    for name in sorted(actual_areas - expected_areas):
        contract_violations.append(f"未登记域 {name} 出现在输入中——冻结契约闭合失败（额外/改名域）")
    blocking.extend(contract_violations)
    blocking.extend(register_violations)

    for area_name in sorted(expected_areas | actual_areas):
        if area_name not in expected_areas:
            areas_report[area_name] = {
                "verdict": "BLOCKED",
                "capabilities": [],
                "problems": ["未登记域——冻结契约闭合失败"],
                "selected_candidate": None,
                "adr": None,
            }
            continue
        cdef = contract[area_name]
        if area_name not in actual_areas:
            areas_report[area_name] = {
                "verdict": "BLOCKED",
                "capabilities": [],
                "problems": ["必需域整体缺失——冻结契约闭合失败（删域不得放行）"],
                "selected_candidate": None,
                "adr": cdef and None,
            }
            continue
        area = areas_in.get(area_name)
        area = area if isinstance(area, dict) else {}
        area_status = "COMPLETE"
        area_problems = []
        caps_report = []

        def block(msg):
            nonlocal area_status
            area_status = "BLOCKED"
            area_problems.append(msg)

        def not_complete(msg):
            nonlocal area_status
            if area_status != "BLOCKED":
                area_status = "NOT_COMPLETE"
            area_problems.append(msg)

        req_by_id, req_dups = _index_caps(area.get("required_capabilities"))
        expected_req = set(cdef["required"])
        for cid in sorted(expected_req - set(req_by_id)):
            block(f"必需能力 {cid} 缺失——冻结契约闭合失败（删项绕过形态，不得放行）")
        for cid in sorted(set(req_by_id) - expected_req):
            block(f"未登记必需能力 {cid}——冻结契约闭合失败（额外/改名项）")
        for cid in req_dups:
            block(f"必需能力 {cid} 重复声明——冻结契约闭合失败")

        for cid in sorted(expected_req & set(req_by_id)):
            cap = req_by_id[cid]
            st = cap.get("status")
            entry = {"id": cid, "status": st}
            if st != "VERIFIED":
                not_complete(f"必需能力 {cid} 状态={st}（非 VERIFIED）——不得以演示/截图证据遮蔽")
                entry["result"] = "FAIL"
            else:
                ev = cap.get("evidence")
                want_path = cdef["required"][cid]
                want = cap.get("sha256")
                p = repo / ev if isinstance(ev, str) and ev else None
                if not isinstance(ev, str) or not ev:
                    block(f"必需能力 {cid} 证据路径缺失（必填）")
                    entry["result"] = "BLOCKED(evidence-missing)"
                elif ev != want_path:
                    block(f"必需能力 {cid} 证据路径与冻结契约不符: {ev}（契约钉住 {want_path}）")
                    entry["result"] = "BLOCKED(evidence-path-contract-mismatch)"
                elif not p.is_file():
                    block(f"必需能力 {cid} 证据文件不存在: {ev}")
                    entry["result"] = "BLOCKED(evidence-missing)"
                elif not isinstance(want, str) or not want.strip():
                    block(f"必需能力 {cid} 缺少 sha256 钉住值（必填——删 hash 不得放行）")
                    entry["result"] = "BLOCKED(sha256-missing)"
                elif not SHA256_RE.match(want):
                    block(f"必需能力 {cid} sha256 非法（须 64 位小写十六进制）: {want!r}")
                    entry["result"] = "BLOCKED(sha256-invalid)"
                elif _sha256(p) != want:
                    block(f"必需能力 {cid} 证据哈希不符: {ev}")
                    entry["result"] = "BLOCKED(evidence-hash-mismatch)"
                else:
                    entry["result"] = "PASS"
            caps_report.append(entry)

        def_by_id, def_dups = _index_caps(area.get("deferred_capabilities"))
        expected_def = set(cdef["deferred"])
        for cid in sorted(expected_def - set(def_by_id)):
            block(f"递延项 {cid} 缺失——冻结契约闭合失败（删递延不得放行）")
        for cid in sorted(set(def_by_id) - expected_def):
            block(f"未登记递延项 {cid}——冻结契约闭合失败（额外/改名项）")
        for cid in def_dups:
            block(f"递延项 {cid} 重复声明——冻结契约闭合失败")

        for cid in sorted(expected_def & set(def_by_id)):
            cap = def_by_id[cid]
            deferred_total += 1
            st = cap.get("status")
            rid = cap.get("risk_id")
            binding = cdef["deferred"][cid]
            want_rid = binding["risk_id"]
            entry = {"id": cid, "status": st, "risk_id": rid}
            r = risk_by_id.get(rid) if isinstance(rid, str) else None
            if st not in DEFERRED_STATUSES:
                block(f"递延项 {cid} 状态={st} 非法（合法递延态：{'/'.join(sorted(DEFERRED_STATUSES))}）")
                entry["result"] = "BLOCKED(deferred-status-invalid)"
            elif rid != want_rid:
                block(f"递延项 {cid} 风险绑定 {rid} 与冻结契约不符（契约钉住 {want_rid}）")
                entry["result"] = "BLOCKED(risk-binding-contract-mismatch)"
            elif not r:
                block(f"递延项 {cid} 引用风险条目 {rid} 不存在")
                entry["result"] = "BLOCKED(risk-missing)"
            elif not _meaningful_str(r.get("resolve_by_stage")) or not _meaningful_str(r.get("failure_handling")):
                # R2 F01：null/对象/数组/数字/空白/占位值一律拒绝，不得 str() 转换冒充。
                block(f"递延项 {cid} 风险条目 {rid} 的 resolve_by_stage/failure_handling 必须是有意义字符串"
                      f"（实际 resolve_by_stage={type(r.get('resolve_by_stage')).__name__}"
                      f"/{str(r.get('resolve_by_stage'))[:20]!r}，"
                      f"failure_handling={type(r.get('failure_handling')).__name__}"
                      f"/{str(r.get('failure_handling'))[:20]!r}）")
                entry["result"] = "BLOCKED(risk-incomplete)"
            elif r.get("status") not in DEFERRED_RISK_STATUS_MAP[st]:
                # R2 F01：输入递延状态与登记状态矛盾（如登记已 CLOSED 仍挂账）不得放行。
                block(f"递延项 {cid} 输入状态={st} 与风险条目 {rid} 登记状态={r.get('status')} 矛盾"
                      f"（{st} 要求登记状态 ∈ {'/'.join(sorted(DEFERRED_RISK_STATUS_MAP[st]))}；"
                      f"已收口/未知登记状态不得支撑递延挂账）")
                entry["result"] = "BLOCKED(risk-status-contradiction)"
            else:
                # R3 F01（G6）：截止阶段与真实阶段索引及冻结契约做机器校验绑定。
                stage_problems = _stage_binding_problems(
                    r["resolve_by_stage"], r.get("resolve_by_stage_id"), r.get("resolve_latest_stage_id"),
                    binding["deadline_stage"], binding["latest_stage"])
                if stage_problems:
                    for _cat, msg in stage_problems:
                        block(f"递延项 {cid} 风险条目 {rid}：{msg}")
                    entry["result"] = f"BLOCKED({stage_problems[0][0]})"
                else:
                    stage_id = r["resolve_by_stage_id"].strip()
                    latest_id = r["resolve_latest_stage_id"].strip()
                    entry["result"] = f"TRACKED({rid} -> {stage_id} 最迟 {latest_id})"
            caps_report.append(entry)

        areas_report[area_name] = {
            "verdict": area_status,
            "capabilities": caps_report,
            "problems": area_problems,
            "selected_candidate": area.get("selected_candidate"),
            "adr": area.get("adr"),
        }
        if area_status != "COMPLETE":
            blocking.append(f"{area_name}: {area_status} — {'; '.join(area_problems)}")

    if blocking:
        verdict = "NO-GO"
        shell_replacement = "BLOCKED: 存在未真实验证的高风险必需能力/不可信证据/冻结契约闭合违例，替壳路径明确阻塞"
    elif deferred_total:
        verdict = "PASS_WITH_CONDITIONS"
        shell_replacement = ("ALLOWED_FOR_NEXT_STAGE_ONLY: 实施平台高风险必需能力均有真实原型证据；"
                             f"{deferred_total} 个递延项已全部挂账（截止阶段+失败处理），跨平台/授权态放行挂 R09/R10 强制关卡")
    else:
        verdict = "PASS"
        shell_replacement = "ALLOWED_FOR_NEXT_STAGE_ONLY"

    report = {
        "verdict": verdict,
        "shell_replacement_path": shell_replacement,
        "implementation_platform": inputs.get("implementation_platform"),
        "contract": {
            "version": CONTRACT_VERSION,
            "authority": "FROZEN_CONTRACT（r01_t08_gate_check.py 内冻结，独立于被验输入）",
            "domains": sorted(contract),
        },
        "contract_violations": contract_violations,
        "register_violations": register_violations,
        "areas": areas_report,
        "deferred_count": deferred_total,
        "blocking_reasons": blocking,
    }
    return verdict, report


def self_test():
    inputs_real = _load(DEFAULT_INPUTS)
    register = _load(DEFAULT_REGISTER)
    results = []

    v, r = evaluate(copy.deepcopy(inputs_real), register)
    results.append(("positive-control",
                    v in ("PASS", "PASS_WITH_CONDITIONS") and r["areas"]["browser_host"]["verdict"] == "COMPLETE",
                    f"真实 R01 数据 verdict={v}（browser_host={r['areas']['browser_host']['verdict']}，deferred={r['deferred_count']}）"))

    # N1（A15 负向核心）：截图通过但用户接管失败
    m = copy.deepcopy(inputs_real)
    for cap in m["areas"]["browser_host"]["required_capabilities"]:
        if cap["id"] == "user_takeover":
            cap["status"] = "FAILED"
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "NOT_COMPLETE"
          and any("user_takeover" in p for p in bh["problems"])
          and "BLOCKED" in r["shell_replacement_path"])
    results.append(("N1-screenshot-pass-takeover-fail", ok,
                    f"verdict={v} browser_host={bh['verdict']} shell_path={r['shell_replacement_path'][:60]}…"))

    # N2：证据哈希被篡改 → BLOCKED
    m = copy.deepcopy(inputs_real)
    m["areas"]["pdf_renderer"]["required_capabilities"][0]["sha256"] = "0" * 64
    v, r = evaluate(m, register)
    ok = v == "NO-GO" and r["areas"]["pdf_renderer"]["verdict"] == "BLOCKED"
    results.append(("N2-evidence-hash-tampered", ok,
                    f"verdict={v} pdf_renderer={r['areas']['pdf_renderer']['verdict']}"))

    # N3：递延项风险条目在登记簿被整條删除（挂账消失）→ BLOCKED(risk-missing)
    m = copy.deepcopy(inputs_real)
    reg = copy.deepcopy(register)
    reg["risks"] = [x for x in reg["risks"] if x["id"] != "RR-T05-X1"]
    v, r = evaluate(m, reg)
    ok = (v == "NO-GO" and r["areas"]["browser_host"]["verdict"] == "BLOCKED"
          and any("risk-missing" in c.get("result", "") for c in r["areas"]["browser_host"]["capabilities"]))
    results.append(("N3-untracked-deferred", ok,
                    f"verdict={v} browser_host={r['areas']['browser_host']['verdict']}"))

    # N4：必需能力标 UNVERIFIED（演示遮蔽形态）→ NOT_COMPLETE + NO-GO
    m = copy.deepcopy(inputs_real)
    m["areas"]["storage_cutover"]["required_capabilities"][0]["status"] = "UNVERIFIED"
    v, r = evaluate(m, register)
    ok = v == "NO-GO" and r["areas"]["storage_cutover"]["verdict"] == "NOT_COMPLETE"
    results.append(("N4-required-unverified", ok,
                    f"verdict={v} storage_cutover={r['areas']['storage_cutover']['verdict']}"))

    # N5（F01 修复核心）：删掉 browser_host.user_takeover 整条必需能力 → NO-GO/BLOCKED 且点名缺失
    m = copy.deepcopy(inputs_real)
    m["areas"]["browser_host"]["required_capabilities"] = [
        c for c in m["areas"]["browser_host"]["required_capabilities"] if c["id"] != "user_takeover"]
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "BLOCKED"
          and any("user_takeover" in p and "缺失" in p for p in bh["problems"]))
    results.append(("N5-deleted-user-takeover-rejected", ok,
                    f"verdict={v} browser_host={bh['verdict']}"))

    # N6：删掉整个 browser_host 域 → NO-GO，contract_violations 点名
    m = copy.deepcopy(inputs_real)
    del m["areas"]["browser_host"]
    v, r = evaluate(m, register)
    ok = (v == "NO-GO"
          and any("browser_host" in x and "缺失" in x for x in r["contract_violations"])
          and r["areas"]["browser_host"]["verdict"] == "BLOCKED")
    results.append(("N6-deleted-domain-rejected", ok,
                    f"verdict={v} violations={len(r['contract_violations'])}"))

    # N7：删掉必需能力的 sha256 字段 → NO-GO/BLOCKED(sha256-missing)
    m = copy.deepcopy(inputs_real)
    del m["areas"]["pdf_renderer"]["required_capabilities"][0]["sha256"]
    v, r = evaluate(m, register)
    pr = r["areas"]["pdf_renderer"]
    ok = (v == "NO-GO" and pr["verdict"] == "BLOCKED"
          and any("sha256-missing" in c.get("result", "") for c in pr["capabilities"]))
    results.append(("N7-deleted-sha256-rejected", ok,
                    f"verdict={v} pdf_renderer={pr['verdict']}"))

    # N8：删掉一项已挂账递延项 → NO-GO/BLOCKED
    m = copy.deepcopy(inputs_real)
    m["areas"]["shell_capabilities"]["deferred_capabilities"] = \
        m["areas"]["shell_capabilities"]["deferred_capabilities"][1:]
    v, r = evaluate(m, register)
    sc = r["areas"]["shell_capabilities"]
    ok = (v == "NO-GO" and sc["verdict"] == "BLOCKED"
          and any("recording_authorized_path" in p and "缺失" in p for p in sc["problems"]))
    results.append(("N8-deleted-deferred-rejected", ok,
                    f"verdict={v} shell_capabilities={sc['verdict']}"))

    # N9：额外未登记域 → NO-GO
    m = copy.deepcopy(inputs_real)
    m["areas"]["stealth_domain"] = {"required_capabilities": [], "deferred_capabilities": []}
    v, r = evaluate(m, register)
    ok = (v == "NO-GO"
          and any("stealth_domain" in x for x in r["contract_violations"])
          and r["areas"]["stealth_domain"]["verdict"] == "BLOCKED")
    results.append(("N9-extra-domain-rejected", ok,
                    f"verdict={v} violations={len(r['contract_violations'])}"))

    # N10：必需能力改名 → NO-GO（原名缺失 + 新名未登记双违例）
    m = copy.deepcopy(inputs_real)
    m["areas"]["browser_host"]["required_capabilities"][4]["id"] = "user_takeover_renamed"
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "BLOCKED"
          and any("user_takeover" in p and "缺失" in p for p in bh["problems"])
          and any("user_takeover_renamed" in p for p in bh["problems"]))
    results.append(("N10-renamed-capability-rejected", ok,
                    f"verdict={v} browser_host={bh['verdict']}"))

    # N11：必需能力重复声明 → NO-GO
    m = copy.deepcopy(inputs_real)
    m["areas"]["browser_host"]["required_capabilities"].append(
        copy.deepcopy(m["areas"]["browser_host"]["required_capabilities"][4]))
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "BLOCKED"
          and any("重复" in p and "user_takeover" in p for p in bh["problems"]))
    results.append(("N11-duplicate-capability-rejected", ok,
                    f"verdict={v} browser_host={bh['verdict']}"))

    # N12：递延项非法状态（VERIFIED 伪装已解决）→ NO-GO
    m = copy.deepcopy(inputs_real)
    m["areas"]["browser_host"]["deferred_capabilities"][0]["status"] = "VERIFIED"
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "BLOCKED"
          and any("deferred-status-invalid" in c.get("result", "") for c in bh["capabilities"]))
    results.append(("N12-deferred-invalid-status-rejected", ok,
                    f"verdict={v} browser_host={bh['verdict']}"))

    # N13：递延项改挂另一条真实存在的风险条目 → NO-GO（契约钉住绑定）
    m = copy.deepcopy(inputs_real)
    m["areas"]["browser_host"]["deferred_capabilities"][0]["risk_id"] = "RR-T06-MIC"
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "BLOCKED"
          and any("risk-binding-contract-mismatch" in c.get("result", "") for c in bh["capabilities"]))
    results.append(("N13-deferred-risk-rebound-rejected", ok,
                    f"verdict={v} browser_host={bh['verdict']}"))

    # N14：证据改指另一真实文件并按其重算 sha256 → NO-GO（契约钉住证据路径）
    m = copy.deepcopy(inputs_real)
    alt = "artifacts/rust-tauri/R01/T04/chromium-iso/summary.json"
    m["areas"]["browser_host"]["required_capabilities"][0]["evidence"] = alt
    m["areas"]["browser_host"]["required_capabilities"][0]["sha256"] = _sha256(REPO / alt)
    v, r = evaluate(m, register)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "BLOCKED"
          and any("evidence-path-contract-mismatch" in c.get("result", "") for c in bh["capabilities"]))
    results.append(("N14-evidence-rerouted-rejected", ok,
                    f"verdict={v} browser_host={bh['verdict']}"))

    # N15：areas 整体清空 → NO-GO（五域全缺失）
    m = copy.deepcopy(inputs_real)
    m["areas"] = {}
    v, r = evaluate(m, register)
    ok = (v == "NO-GO" and len(r["contract_violations"]) == len(FROZEN_CONTRACT)
          and all(r["areas"][d]["verdict"] == "BLOCKED" for d in FROZEN_CONTRACT))
    results.append(("N15-emptied-areas-rejected", ok,
                    f"verdict={v} violations={len(r['contract_violations'])}"))

    # N16：风险条目存在但 resolve_by_stage 置空 → BLOCKED(risk-incomplete)
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = ""
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    bh = r["areas"]["browser_host"]
    ok = (v == "NO-GO" and bh["verdict"] == "BLOCKED"
          and any("risk-incomplete" in c.get("result", "") for c in bh["capabilities"]))
    results.append(("N16-risk-entry-incomplete-rejected", ok,
                    f"verdict={v} browser_host={bh['verdict']}"))

    # ---- R01 阶段验收 R2 F01：字段类型/占位值/状态一致性/登记完整性负向 ----

    def _risk_variant(tag, rid, mutate, want, note):
        reg2 = copy.deepcopy(register)
        for x in reg2["risks"]:
            if x["id"] == rid:
                mutate(x)
        vv, rr = evaluate(copy.deepcopy(inputs_real), reg2)
        hit = any(want in c.get("result", "")
                  for a in rr["areas"].values() for c in a["capabilities"])
        results.append((tag, vv == "NO-GO" and hit, f"verdict={vv} want={want} {note}"))

    # N17-N22：resolve_by_stage / failure_handling 的非字符串与占位形态
    _risk_variant("N17-resolve-by-stage-null", "RR-T05-X1",
                  lambda x: x.update(resolve_by_stage=None), "risk-incomplete", "JSON null")
    _risk_variant("N18-failure-handling-array", "RR-T05-X1",
                  lambda x: x.update(failure_handling=[]), "risk-incomplete", "JSON []")
    _risk_variant("N19-resolve-by-stage-object", "RR-T05-X1",
                  lambda x: x.update(resolve_by_stage={}), "risk-incomplete", "JSON {}")
    _risk_variant("N20-failure-handling-blank", "RR-T05-X1",
                  lambda x: x.update(failure_handling="   "), "risk-incomplete", "空白字符串")
    _risk_variant("N21-resolve-by-stage-number", "RR-T05-X1",
                  lambda x: x.update(resolve_by_stage=42), "risk-incomplete", "JSON 数字")
    _risk_variant("N22-failure-handling-placeholder", "RR-T05-X1",
                  lambda x: x.update(failure_handling="TBD"), "risk-incomplete", "占位值")

    # N23-N25：递延输入状态与登记状态矛盾（含已收口形态）
    _risk_variant("N23-risk-closed-vs-deferred", "RR-T05-X1",
                  lambda x: x.update(status="CLOSED"), "risk-status-contradiction",
                  "登记 CLOSED vs 输入 UNVERIFIED")
    _risk_variant("N24-risk-closed-in-r01-vs-deferred", "RR-T05-X1",
                  lambda x: x.update(status="CLOSED_IN_R01"), "risk-status-contradiction",
                  "登记 CLOSED_IN_R01 vs 输入 UNVERIFIED")
    _risk_variant("N25-registered-defect-status-mismatch", "RR-T07-PROD-DEFECT-1",
                  lambda x: x.update(status="OPEN"), "risk-status-contradiction",
                  "登记 OPEN vs 输入 REGISTERED_DEFECT（要求 REGISTERED）")

    # N26-N28：登记本体完整性（重复 id / 缺 id / risks 非列表）
    reg = copy.deepcopy(register)
    reg["risks"].append(copy.deepcopy(reg["risks"][0]))
    dup_id = reg["risks"][0]["id"]
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    ok = (v == "NO-GO"
          and any(dup_id in x and "重复" in x for x in r["register_violations"]))
    results.append(("N26-duplicate-risk-id-rejected", ok,
                    f"verdict={v} register_violations={len(r['register_violations'])}"))

    reg = copy.deepcopy(register)
    reg["risks"].append({"status": "OPEN", "resolve_by_stage": "R09", "failure_handling": "x"})
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    ok = (v == "NO-GO" and any("缺合法 id" in x for x in r["register_violations"]))
    results.append(("N27-risk-entry-missing-id-rejected", ok,
                    f"verdict={v} register_violations={len(r['register_violations'])}"))

    reg = copy.deepcopy(register)
    reg["risks"] = {}
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    ok = (v == "NO-GO" and any("非列表" in x for x in r["register_violations"]))
    results.append(("N28-risks-non-list-rejected", ok,
                    f"verdict={v} register_violations={len(r['register_violations'])}"))

    # ---- R01 阶段验收 R3 F01：截止阶段绑定负向（G6，结构化坐标 × 阶段索引 × 契约）----

    def _stage_variant(tag, rid, mutate, want, note):
        reg2 = copy.deepcopy(register)
        for x in reg2["risks"]:
            if x["id"] == rid:
                mutate(x)
        vv, rr = evaluate(copy.deepcopy(inputs_real), reg2)
        hit = any(want in c.get("result", "")
                  for a in rr["areas"].values() for c in a["capabilities"])
        results.append((tag, vv == "NO-GO" and hit, f"verdict={vv} want={want} {note}"))

    # N29：R3 评审原样反例——正文改成不存在的 R99（结构化坐标仍 R09）
    # → 正文引用不存在阶段（主类别）+ 与结构化坐标不一致，双重拒绝
    _stage_variant("N29-stage-text-nonexistent", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R99（不存在的阶段）"),
                   "risk-stage-unknown", "正文 R99 不在 R00–R11")
    # N30：正文与结构化坐标都改成 R99 → 阶段不存在拒
    _stage_variant("N30-stage-both-nonexistent", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R99", resolve_by_stage_id="R99"),
                   "risk-stage-unknown", "双字段 R99 不在 R00–R11")
    # N31：正文含糊“以后”（无阶段坐标）→ 拒
    _stage_variant("N31-stage-vague-text", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="以后再说"),
                   "risk-stage-vague", "无阶段坐标")
    # N32：截止阶段改成真实但过晚的阶段（R04 契约 → R10/R10 一致拖延）→ 错误截止期拒
    _stage_variant("N32-stage-deadline-too-late", "RR-T02-F1",
                   lambda x: x.update(resolve_by_stage="R10（拖延）", resolve_by_stage_id="R10",
                                      resolve_latest_stage_id="R10"),
                   "risk-stage-deadline-mismatch", "契约 R04，改 R10")
    # N33：错误归属——浏览器边界风险挂到 R02 存储阶段 → 拒
    _stage_variant("N33-stage-wrong-attribution", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R02", resolve_by_stage_id="R02"),
                   "risk-stage-deadline-mismatch", "契约 R09，改 R02")
    # N34：越过适用最迟关卡（MIC 契约 R09 → 最迟 R10）→ 拒
    _stage_variant("N34-stage-latest-past-gate", "RR-T06-MIC",
                   lambda x: x.update(resolve_latest_stage_id="R10"),
                   "risk-stage-latest-mismatch", "契约最迟 R09")
    # N35：越过登记自身声明的最迟关卡（PROXY「R09 最迟 R10」→ R11，正文与字段一致改）
    # → 拒
    _stage_variant("N35-stage-latest-beyond-declared", "RR-T06-PROXY",
                   lambda x: x.update(resolve_by_stage="R09 最迟 R11", resolve_latest_stage_id="R11"),
                   "risk-stage-latest-mismatch", "契约最迟 R10")
    # N36：正文/结构化截止阶段不一致 → 拒
    _stage_variant("N36-stage-text-field-diverge", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage_id="R10"),
                   "risk-stage-text-mismatch", "正文 R09 vs 字段 R10")
    # N37：截止阶段等于当前阶段（不晚于 R01）→ 拒
    _stage_variant("N37-stage-current-stage", "RR-T02-F1",
                   lambda x: x.update(resolve_by_stage="R01", resolve_by_stage_id="R01"),
                   "risk-stage-current-or-past", "R01 非后续阶段")
    # N38：删结构化截止阶段字段 → 拒
    _stage_variant("N38-stage-field-deleted", "RR-T05-X1",
                   lambda x: x.pop("resolve_by_stage_id", None),
                   "risk-stage-field-missing", "缺 resolve_by_stage_id")
    # N39：删最迟关卡字段 → 拒
    _stage_variant("N39-stage-latest-field-deleted", "RR-T05-X1",
                   lambda x: x.pop("resolve_latest_stage_id", None),
                   "risk-stage-field-missing", "缺 resolve_latest_stage_id")
    # N40：最迟关卡早于截止阶段（倒挂）→ 拒
    _stage_variant("N40-stage-order-inverted", "RR-T07-F2",
                   lambda x: x.update(resolve_latest_stage_id="R07"),
                   "risk-stage-order-inverted", "deadline R08 > latest R07")
    # N41：结构化坐标非字符串（null）→ 拒
    _stage_variant("N41-stage-field-null", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage_id=None),
                   "risk-stage-field-missing", "JSON null")
    # N42：正文「最迟」标记与结构化最迟关卡不符 → 拒
    _stage_variant("N42-stage-latest-marker-diverge", "RR-T06-PROXY",
                   lambda x: x.update(resolve_by_stage="R09 最迟 R11"),
                   "risk-stage-text-mismatch", "最迟 R11 vs 字段 R10")
    # N43：正文夹带第二处不存在阶段 token → 拒（不只看首个 token）
    _stage_variant("N43-stage-hidden-nonexistent-token", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（等 R99 再议）"),
                   "risk-stage-unknown", "正文内嵌 R99")

    # ---- R01 阶段验收 R4 F01：完整标识边界负向（G6b，伪装/截断形态拒绝）----

    # N44：R4 评审原样反例——正文改成更长的 R099（结构化坐标仍 R09/R09 不动）。
    # 旧 R\d{2} 把 R099 截成 R09 放行（exit 0/PASS_WITH_CONDITIONS）；完整形态
    # 提取后 R099 不在阶段索引 → unknown + 首坐标 mismatch 双重拒绝
    _stage_variant("N44-stage-text-r099", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R099（不存在的阶段）"),
                   "risk-stage-unknown", "正文 R099 完整提取不截断")
    # N45：混合真实与伪造（R09/R099）→ 完整提取出 R099 → 拒
    _stage_variant("N45-stage-text-mixed-r09-r099", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09/R099（混合了伪造阶段）"),
                   "risk-stage-unknown", "混合 R09/R099")
    # N46：前缀伪装 XR09（无完整形态引用，形似出现被检出）→ disguised 拒
    _stage_variant("N46-stage-text-prefixed-xr09", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="XR09（伪造阶段）"),
                   "risk-stage-disguised", "前缀伪装 XR09")
    # N47：后缀伪装 R09X → disguised 拒
    _stage_variant("N47-stage-text-suffixed-r09x", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09X（后缀伪装阶段）"),
                   "risk-stage-disguised", "后缀伪装 R09X")
    # N48：「最迟 R099」——最迟标记按完整形态捕获，R099 不在索引 → 拒
    _stage_variant("N48-stage-latest-marker-r099", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09 最迟 R099"),
                   "risk-stage-unknown", "最迟标记 R099 完整形态")
    # N49：四位伪装 R0999 → 完整提取不截断 → 拒
    _stage_variant("N49-stage-text-r0999", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R0999（多一位伪装阶段）"),
                   "risk-stage-unknown", "R0999 不在 R00–R11")
    # N50：下划线嵌入 R09_stage（完整边界含下划线）→ disguised 拒
    _stage_variant("N50-stage-text-underscore-suffix", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09_stage（下划线伪装阶段）"),
                   "risk-stage-disguised", "下划线伪装 R09_stage")

    # ---- R01 阶段修复 R5 F01：复合/小阶段/拼接形态负向（G6b 边界扩展）----
    # 旧 R4 边界类 [A-Za-z0-9_] 把点号/连字符当合法边界：正文 R09.5/R09-5/R09．5
    # 被截成 R09 放行（R5 评审实测 exit 0/PASS_WITH_CONDITIONS）。R5 把点号/
    # 连字符族列入标识延续字符：这类出现按完整复合形态报 risk-stage-disguised，
    # 不再产生合法 R09 引用。

    # N51：R5 评审原样反例——正文 R09.5（点号虚构小阶段）→ disguised 拒
    _stage_variant("N51-stage-text-dot-substage", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09.5（虚构小阶段）"),
                   "risk-stage-disguised", "点号小阶段 R09.5 完整复合形态")
    # N52：连字符虚构小阶段 R09-5 → disguised 拒
    _stage_variant("N52-stage-text-hyphen-substage", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09-5（虚构小阶段）"),
                   "risk-stage-disguised", "连字符小阶段 R09-5")
    # N53：全角点虚构小阶段 R09．5 → disguised 拒
    _stage_variant("N53-stage-text-fullwidth-dot-substage", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09．5（虚构小阶段）"),
                   "risk-stage-disguised", "全角点小阶段 R09．5")
    # N54：全角连字符虚构小阶段 R09－5 → disguised 拒
    _stage_variant("N54-stage-text-fullwidth-hyphen-substage", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09－5（虚构小阶段）"),
                   "risk-stage-disguised", "全角连字符小阶段 R09－5")
    # N55：多级虚构小阶段 R09.5.1 → disguised 拒
    _stage_variant("N55-stage-text-multi-level-substage", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09.5.1（多级虚构小阶段）"),
                   "risk-stage-disguised", "多级小阶段 R09.5.1")
    # N56：点号拼接两个阶段标识 R09.R10（拼接/歧义写法）→ disguised 拒
    _stage_variant("N56-stage-text-concat-two-stages", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09.R10（点号拼接阶段标识）"),
                   "risk-stage-disguised", "点号拼接 R09.R10")
    # N57：最迟标记位复合形态「最迟 R09.5」→ 标记按完整形态不匹配，复合出现拒
    _stage_variant("N57-stage-latest-marker-dot-substage", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09 最迟 R09.5"),
                   "risk-stage-disguised", "最迟标记复合 R09.5")
    # N58：真实阶段与虚构小阶段混合 R09/R09.5 → disguised 拒（真实 R09 保留但复合出现即拒）
    _stage_variant("N58-stage-text-mixed-real-compound", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09/R09.5（混合真实阶段与虚构小阶段）"),
                   "risk-stage-disguised", "混合 R09/R09.5")

    # ---- R01 阶段验收 R6 F01：全角同形/大小写形态负向（G6c 全角折叠 + 伪装检出扩展）----
    # R5 及以前 STAGE_TOKEN_RE/STAGE_DISGUISE_RE/LATEST_MARKER_RE 只从 ASCII R 开始
    # 寻找：全角同形阶段（Ｒ９９/Ｒ１０/Ｒ０９）与混合宽度形态（Ｒ99/R９９）完全
    # 不参加提取、真实阶段索引存在性、正文首位与「最迟」一致性核验——与合法 ASCII
    # R09 混用即 exit 0/PASS_WITH_CONDITIONS（R6 评审真实 CLI 实测），使人工交接
    # 截止与机器判定相矛盾。R6 修复：正文提取前 1:1 等长折叠全角拉丁字母/数字为
    # ASCII，折叠后走同一套完整标识语法与一致性核验；小写 r+数字按大小写契约
    # （合法引用仅认大写 R）由 IGNORECASE 伪装检出单独拒绝。

    # N59：R6 评审原样反例 1——正文首位全角 Ｒ９９（不存在的截止阶段）与合法
    # R09 混用；折叠后 R99 完整提取 → 不存在 + 首坐标不一致双重拒绝
    _stage_variant("N59-stage-text-fullwidth-r99-first", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="Ｒ９９（不存在的截止阶段）；R09（仅用于宿主集成）"),
                   "risk-stage-unknown", "全角 Ｒ９９ 折叠为 R99，不在 R00–R11")
    # N60：R6 评审原样反例 2——「最迟 Ｒ１０」全角虚构放宽关卡；折叠后标记捕获
    # R10 ≠ 结构化最迟 R09 → 拒
    _stage_variant("N60-stage-latest-fullwidth-r10", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 Ｒ１０（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "最迟 Ｒ１０ 折叠为 R10 vs 结构化 R09")
    # N61：正文首位全角 Ｒ１０（真实存在但非本条截止阶段）伪装放宽 → 折叠后首坐标
    # R10 ≠ 结构化 R09 → 拒
    _stage_variant("N61-stage-text-fullwidth-r10-first", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="Ｒ１０（虚构放宽关卡）；R09（宿主集成）"),
                   "risk-stage-text-mismatch", "全角 Ｒ１０ 折叠为首坐标 R10")
    # N62：混合宽度 Ｒ99（全角 R + ASCII 数字）与合法 R09 混用 → 折叠 R99 → 拒
    _stage_variant("N62-stage-text-fullwidth-r-ascii-digit", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="Ｒ99（不存在的截止阶段）；R09（校验）"),
                   "risk-stage-unknown", "混合宽度 Ｒ99 折叠为 R99")
    # N63：全角前缀复合 ＸＲ０９（ASCII XR09 的全角同族）与合法 R09 混用 → 折叠后
    # 按完整复合形态拒（与 ASCII XR09 一致）
    _stage_variant("N63-stage-text-fullwidth-compound-xr09", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="ＸＲ０９（全角前缀伪装）；R09（校验）"),
                   "risk-stage-disguised", "ＸＲ０９ 折叠为 XR09 复合形态")
    # N64：小写 r99 与合法 R09 混用（R6 评审记录的大小写同族误放）→ 大小写契约
    # 仅认大写 R，小写形态按伪装拒
    _stage_variant("N64-stage-text-lowercase-r99", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="r99（不存在的截止阶段）；R09（仅用于宿主集成）"),
                   "risk-stage-disguised", "小写 r99 不是合法引用（大小写契约）")
    # N65：全角小写 ｒ９９ 与合法 R09 混用 → 折叠为 r99 后按伪装拒
    _stage_variant("N65-stage-text-fullwidth-lowercase-r99", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="ｒ９９（不存在的截止阶段）；R09（校验）"),
                   "risk-stage-disguised", "全角小写 ｒ９９ 折叠为 r99")

    # P2（正向规范化对照，R6 评审第三行期望）：正文首个全角 Ｒ０９ 经规范化即 R09，
    # 与结构化坐标一致——折叠后不再被忽略，首坐标校验不再失真；真实一致的正文
    # 不得因书写宽度被误伤（方案 a：先规范化再检验，非一律拒绝全角书写）。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "Ｒ０９（看似合法的截止）；R09（校验）"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P2-positive-fullwidth-normalized",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 全角 Ｒ０９ 规范化后与结构化 R09 一致（TRACKED 命中={hit}）"))

    # ---- R01 阶段验收 R7 F01：重复「最迟」标记逐处核验负向（G6d，失败关闭）----
    # R6 及以前 LATEST_MARKER_RE.search() 只校验第一处「最迟」——正文
    # 「最迟 R09；最迟 R10」（结构化最迟 R09）的第二处相矛盾期限被完全忽略
    # （R7 评审真实 CLI 实测 exit 0/PASS_WITH_CONDITIONS）。R7 修复为 finditer
    # 逐处核验：任一「最迟 X」引用不存在阶段 → unknown；任一 ≠ 结构化 latest →
    # text-mismatch；多处全部一致方可放行；与「最迟」语法无关的正常阶段提及不误伤。

    # N66：R7 评审原样反例——首处一致/后处冲突「最迟 R09；最迟 R10」（结构化最迟
    # R09）→ 第二处 R10 ≠ R09，拒
    _stage_variant("N66-stage-latest-double-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "第二处最迟 R10 ≠ 结构化 R09")
    # N67：顺序交换「最迟 R10；最迟 R09」→ 首处 R10 ≠ R09，拒（方向性对照：
    # 不是"只查首处"换方向重现，而是每一处都核验）
    _stage_variant("N67-stage-latest-double-reversed", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 R10；最迟 R09"),
                   "risk-stage-text-mismatch", "首处最迟 R10 ≠ 结构化 R09")
    # N68：全角混用「最迟 Ｒ０９；最迟 Ｒ１０」→ 折叠后逐处核验，第二处 R10 拒
    _stage_variant("N68-stage-latest-double-fullwidth-mixed", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 Ｒ０９；最迟 Ｒ１０"),
                   "risk-stage-text-mismatch", "全角 Ｒ１０ 折叠为 R10，第二处与结构化 R09 矛盾")

    # P3（多处一致放行对照，R7 评审第七行）：「最迟 R09；最迟 R09」两处均与结构化
    # latest R09 一致——清晰契约：正文每一处「最迟」都须等于结构化最迟关卡，全部
    # 一致即放行，重复但一致的书写不得被误伤。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09（宿主集成）最迟 R09；最迟 R09（重复但一致）"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P3-positive-double-latest-consistent",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 两处最迟 R09 均与结构化一致（TRACKED 命中={hit}）"))

    # P4（正常上下文提及 R10 不误伤对照，R7 评审第六行）：「；R10（后续平台事项）
    # 最迟 R09」——R10 是与「最迟」语法无关的正常上下文提及（真实存在的阶段、非
    # 最迟标记），唯一最迟标记 R09 与结构化一致 → 放行，不得因正文出现 R10 字样误拒。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09（宿主集成）；R10（后续平台事项）最迟 R09"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P4-positive-contextual-r10-not-marker",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 上下文提及 R10 非最迟标记不误伤（TRACKED 命中={hit}）"))

    # ---- R01 阶段修复 R8 F01：显式期限连接形态负向/正向（G6d 连接形态扩展）----
    # R7 标记语法只识别「最迟」后接空白再紧跟阶段 ID：「最迟：R10」「最迟: R10」
    # 「最迟于 R10」「最迟为 R10」等常见自然书写（及同族否定连接式「不晚于
    # R10」）是同样明确的期限声明却完全不构成标记——结构化最迟 R09 时正文写出
    # 放宽到 R10 的期限被 exit 0 误放（R8 评审真实 CLI 实测，通用阶段扫描只查
    # R10 存在性、不与结构化最迟比较）。R8 修复：标记 = 触发词族 + 有界连接段
    # + 完整阶段 ID，逐处核验；触发词在子句内解析不出阶段 ID →
    # risk-stage-latest-unresolved 失败关闭。

    # N69：R8 评审原样反例——第二处「最迟：R10」（全角冒号）与结构化最迟 R09 矛盾
    _stage_variant("N69-stage-latest-fullwidth-colon-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 R09；最迟：R10（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "全角冒号连接的第二处最迟 R10 ≠ 结构化 R09")
    # N70：全角冒号 + 全角阶段「最迟：Ｒ１０」→ 折叠后标记捕获 R10 → 拒
    _stage_variant("N70-stage-latest-fullwidth-colon-fullwidth-stage", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 R09；最迟：Ｒ１０（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "全角冒号+全角 Ｒ１０ 折叠 R10 ≠ 结构化 R09")
    # N71：连接词「最迟于 R10」→ 标记捕获 → 拒
    _stage_variant("N71-stage-latest-yu-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 R09；最迟于 R10（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "「最迟于」连接的 R10 ≠ 结构化 R09")
    # N72：单处冒号形式「最迟：R10」（无第一处一致掩护）→ 拒
    _stage_variant("N72-stage-latest-single-colon-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟：R10（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "单处「最迟：R10」≠ 结构化 R09")
    # N73：半角冒号「最迟: R10」→ 拒
    _stage_variant("N73-stage-latest-halfwidth-colon-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟: R10（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "半角冒号连接的 R10 ≠ 结构化 R09")
    # N74：连接词族代表「最迟为 R10」→ 有界连接段覆盖非枚举连接词 → 拒
    _stage_variant("N74-stage-latest-wei-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟为 R10（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "「最迟为」连接的 R10 ≠ 结构化 R09")
    # N75：同族否定连接式「不晚于 R10」→ 同为显式期限声明，拒
    _stage_variant("N75-stage-latest-not-later-than-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）不晚于 R10（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "「不晚于 R10」≠ 结构化最迟 R09")
    # N76：否定连接式 + 全角「不得迟于 Ｒ１０」→ 折叠后拒
    _stage_variant("N76-stage-latest-must-not-delay-fullwidth-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成），不得迟于 Ｒ１０（虚构放宽关卡）"),
                   "risk-stage-text-mismatch", "「不得迟于 Ｒ１０」折叠 R10 ≠ 结构化 R09")
    # N77：触发词解析不出阶段 ID（自然语言期限「最迟于第三阶段完成」）→ 失败关闭拒
    _stage_variant("N77-stage-latest-unresolved-trigger", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟于第三阶段完成"),
                   "risk-stage-latest-unresolved", "触发词「最迟」无法解析出阶段 ID，失败关闭")

    # P5（连接形态一致正向）：「最迟：R09；最迟于 R09」两处不同连接形态均与结构化
    # latest R09 一致——清晰契约放行，一致的自然书写连接形态不得被误伤。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09（宿主集成）最迟：R09；最迟于 R09（重复但一致）"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P5-positive-connector-forms-consistent",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 全角冒号/连接词形态最迟 R09 均一致（TRACKED 命中={hit}）"))

    # P6（否定连接式一致正向）：「不晚于 R09 完成」与结构化 latest R09 一致——
    # 触发词族一致书写放行，不得因引入否定连接式误伤一致期限。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09（宿主集成）不晚于 R09 完成"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P6-positive-negative-trigger-consistent",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 「不晚于 R09」与结构化一致（TRACKED 命中={hit}）"))

    # ---- R01 阶段修复 R9 F01：单个期限子句内第二阶段负向（G6e 期限子句唯一一致）----
    # R8 标记语法把触发词绑定到其后首个阶段 ID，同一子句内首个阶段之后的第二个
    # 阶段——改期（「（原定 R09，现改 R10）完成」）、顺延（「由原定的 R09 顺延至
    # R10」）、选择（「在 R09 或 R10」）、斜杠并列（「R09/R10」及全角变体）——
    # 被通用阶段扫描当普通上下文只查存在性，明确改晚的强制期限仍 exit 0 误放
    # （R9 评审真实 CLI 实测，结构化坐标保持 R09/R09）。R9 修复：每个触发词治理
    # 「从触发词起到子句终结符止」的完整区域，区域内每个完整阶段引用都必须与
    # 结构化 latest 一致——出现任何第二阶段即无法机器判定唯一一致期限，失败关闭
    # 拒绝；不枚举改期/顺延/选择语言（中文「或」、拉丁「or」、半/全角斜杠及
    # 组合由「区域内出现第二阶段」统一覆盖）。

    # N78：R9 评审原样反例 1——改期「最迟（原定 R09，现改 R10）完成」
    _stage_variant("N78-stage-latest-clause-correction", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟（原定 R09，现改 R10）完成"),
                   "risk-stage-text-mismatch", "同一子句内改期至 R10，第二阶段与结构化 R09 矛盾")
    # N79：R9 评审原样反例 2——顺延「最迟由原定的 R09 顺延至 R10 完成」
    _stage_variant("N79-stage-latest-clause-postponed", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟由原定的 R09 顺延至 R10 完成"),
                   "risk-stage-text-mismatch", "同一子句内顺延至 R10，第二阶段与结构化 R09 矛盾")
    # N80：选择「最迟在 R09 或 R10 完成」——可选择更晚期限即非唯一期限
    _stage_variant("N80-stage-latest-clause-or-choice", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟在 R09 或 R10 完成"),
                   "risk-stage-text-mismatch", "「或」引入第二阶段，唯一一致期限无法判定")
    # N81：斜杠并列「最迟 R09/R10 完成」——非唯一期限被首阶段代表
    _stage_variant("N81-stage-latest-clause-slash-pair", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 R09/R10 完成"),
                   "risk-stage-text-mismatch", "斜杠并列第二阶段 R10 与结构化 R09 矛盾")
    # N82：全角改期「最迟（原定 Ｒ０９，现改 Ｒ１０）完成」——折叠后同规则拒
    _stage_variant("N82-stage-latest-clause-fullwidth-correction", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟（原定 Ｒ０９，现改 Ｒ１０）完成"),
                   "risk-stage-text-mismatch", "全角 Ｒ１０ 折叠 R10，第二阶段与结构化 R09 矛盾")
    # N83：全角斜杠「最迟 Ｒ０９／Ｒ１０ 完成」——／为合法分隔符，两引用均入区域
    _stage_variant("N83-stage-latest-clause-fullwidth-slash", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟 Ｒ０９／Ｒ１０ 完成"),
                   "risk-stage-text-mismatch", "全角斜杠并列，第二阶段 R10 与结构化 R09 矛盾")
    # N84：拉丁连接「最迟在 R09 or R10 完成」——区域规则语言无关，不枚举连接词
    _stage_variant("N84-stage-latest-clause-latin-or", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟在 R09 or R10 完成"),
                   "risk-stage-text-mismatch", "拉丁 or 引入第二阶段，区域规则统一覆盖")

    # P7（同子句内重复一致正向）：「最迟 R09 完成（R09 复核）」——触发词区域内
    # 的重复阶段引用全部等于结构化 latest R09，唯一一致期限可判定，放行；重复但
    # 一致的书写不得被「第二阶段」规则误伤。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09（宿主集成）最迟 R09 完成（R09 复核）"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P7-positive-clause-repeat-consistent",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 同子句内重复 R09 均与结构化一致（TRACKED 命中={hit}）"))

    # P8（引号词名引用正向，R9 评审披露限制的最小处理）：「R10 文档介绍“最迟”
    # 字段」——第二处触发词被成对引号紧包，是引用词名而非期限声明，不触发解析
    # 义务；R10 为终结符后的普通上下文提及（真实存在）。一致期限不得因引用词名
    # 被误拒。被包裹内容不可能携带阶段 ID，豁免无法隐藏任何期限。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09 最迟 R09 完成；R10 文档介绍“最迟”字段"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P8-positive-quoted-trigger-wordname",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 引号词名引用「“最迟”」非期限声明不误拒（TRACKED 命中={hit}）"))

    # ---- R01 阶段修复 R10 F01：期限区域完整性负向（G6f 词名豁免收窄/括号感知/
    # 最晚入族/前边界拒绝）----
    # R9 的引号紧包豁免 continue 跳过整个触发词，实际移除了完整后续区域的核验
    # 义务——「“最迟”原定 R09，现改 R10 完成」「“最迟”字段：原定 R09，现改
    # R10 完成」（四种支持引号同族）、「“最迟” R09/R10 完成」「「最迟」
    # Ｒ０９／Ｒ１０ 完成」全部 exit 0 误放（R10 评审真实 CLI 实测，结构化坐标
    # 保持 R09/R09）；区域终结符不看括号层次，「最迟（原定 R09；现改 R10）
    # 完成」及括号内换行同形把完整改期声明截断成只含首阶段的区域。R9 修复
    # 报告另称「最晚于 R10 完成」「R10 前必须完成」属文档化边界普通上下文——
    # R10 评审独立裁定已披露的明确截止不得凭文档化边界自动当普通上下文。
    # R10 修复：词名豁免收窄为仅解析义务、区域边界括号感知、「最晚」入触发词
    # 族逐处核验、「前」边界期限按 risk-stage-boundary-unsupported 失败关闭。

    # N85：R10 评审原样反例——引号词名后实际改期「“最迟”原定 R09，现改 R10」
    _stage_variant("N85-stage-quote-wordname-correction", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）“最迟”原定 R09，现改 R10 完成"),
                   "risk-stage-text-mismatch", "词名引用不遮蔽其后实际改期 R10")
    # N86：R10 评审原样反例——字段名引用后实际改期「“最迟”字段：原定 R09，现改 R10」
    _stage_variant("N86-stage-quote-fieldname-correction", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）“最迟”字段：原定 R09，现改 R10 完成"),
                   "risk-stage-text-mismatch", "字段名引用不豁免其后的实际赋值/改期")
    # N87：直角单引号「」字段名同形（R10 quote-pairs 覆盖）
    _stage_variant("N87-stage-quote-fieldname-corner", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）「最迟」字段：原定 R09，现改 R10 完成"),
                   "risk-stage-text-mismatch", "「」词名引用同族不遮蔽改期")
    # N88：双层直角引号『』字段名同形
    _stage_variant("N88-stage-quote-fieldname-double-corner", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）『最迟』字段：原定 R09，现改 R10 完成"),
                   "risk-stage-text-mismatch", "『』词名引用同族不遮蔽改期")
    # N89：ASCII 双引号字段名同形
    _stage_variant("N89-stage-quote-fieldname-ascii", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）\"最迟\"字段：原定 R09，现改 R10 完成"),
                   "risk-stage-text-mismatch", "ASCII 引号词名引用同族不遮蔽改期")
    # N90：引号词名后斜杠非唯一期限「“最迟” R09/R10 完成」
    _stage_variant("N90-stage-quote-wordname-slash-pair", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）“最迟” R09/R10 完成"),
                   "risk-stage-text-mismatch", "词名引用后的非唯一期限不得豁免")
    # N91：引号词名后全角斜杠「「最迟」 Ｒ０９／Ｒ１０ 完成」
    _stage_variant("N91-stage-quote-wordname-fw-slash", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）「最迟」 Ｒ０９／Ｒ１０ 完成"),
                   "risk-stage-text-mismatch", "全角斜杠非唯一期限折叠后同拒")
    # N92：R10 评审原样反例——括号内分号「最迟（原定 R09；现改 R10）完成」
    _stage_variant("N92-stage-paren-semicolon-correction", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟（原定 R09；现改 R10）完成"),
                   "risk-stage-text-mismatch", "括号内分号不截断完整改期声明")
    # N93：括号内换行同形——区域边界括号感知统一覆盖
    _stage_variant("N93-stage-paren-newline-correction", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）最迟（原定 R09\n现改 R10）完成"),
                   "risk-stage-text-mismatch", "括号内换行不截断完整改期声明")
    # N94：R10 评审裁定边界 1——同义词触发词「最晚于 R10 完成」入族逐处核验
    _stage_variant("N94-stage-latest-synonym-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）；最晚于 R10 完成"),
                   "risk-stage-text-mismatch", "「最晚」入触发词族，R10 ≠ 结构化 R09")
    # N95：R10 评审裁定边界 2——前边界期限「R10 前必须完成」失败关闭
    _stage_variant("N95-stage-before-suffix-conflict", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）；R10 前必须完成"),
                   "risk-stage-boundary-unsupported", "阶段锚定「前」边界期限机器不支持即拒")
    # N96：前边界同族泛化「R10 之前完成」——封闭词素族统一覆盖
    _stage_variant("N96-stage-before-suffix-variant", "RR-T05-X1",
                   lambda x: x.update(resolve_by_stage="R09（宿主集成）；R10 之前完成"),
                   "risk-stage-boundary-unsupported", "「之前」同族前边界期限同拒")

    # P9（直角引号词名引用正向）：「R10 文档介绍「最迟」字段」——第二处触发词
    # 被「」紧包，是引用词名而非期限声明，豁免其解析义务；其后续区域
    # 「「最迟」字段」无阶段引用，区域治理平凡通过。R10 为终结符后普通上下文
    # 提及。一致期限不得因引号词名（任意支持引号）被误拒。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09 最迟 R09 完成；R10 文档介绍「最迟」字段"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P9-positive-quoted-wordname-corner",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 「」词名引用非期限声明不误拒（TRACKED 命中={hit}）"))

    # P10（同义词一致正向）：「最晚于 R09 完成」——「最晚」入族后按标记语法
    # 核验，与结构化 latest R09 一致即放行；同义词族不因入族而误伤一致书写。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09（宿主集成）最晚于 R09 完成"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P10-positive-latest-synonym-consistent",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 「最晚于 R09」与结构化一致放行（TRACKED 命中={hit}）"))

    # P11（词名引用后一致提及正向）：「“最迟”字段即 R09」——词名引用豁免解析
    # 义务，但其后续区域仍受区域治理：区域内的阶段引用与结构化 latest 一致
    # （R09 == R09）即放行。词名引用只遮蔽不了「不一致」期限，不误伤一致书写。
    reg = copy.deepcopy(register)
    for x in reg["risks"]:
        if x["id"] == "RR-T05-X1":
            x["resolve_by_stage"] = "R09（宿主集成）“最迟”字段即 R09"
    v, r = evaluate(copy.deepcopy(inputs_real), reg)
    hit = any("TRACKED(RR-T05-X1 -> R09 最迟 R09)" in c.get("result", "")
              for a in r["areas"].values() for c in a["capabilities"])
    results.append(("P11-positive-quoted-consistent-mention",
                    v == "PASS_WITH_CONDITIONS" and hit,
                    f"verdict={v} 词名引用后一致提及 R09 放行（TRACKED 命中={hit}）"))

    all_ok = all(x[1] for x in results)
    for name, passed, detail in results:
        tag = "PASS" if name in ("positive-control", "P2-positive-fullwidth-normalized",
                                 "P3-positive-double-latest-consistent",
                                 "P4-positive-contextual-r10-not-marker",
                                 "P5-positive-connector-forms-consistent",
                                 "P6-positive-negative-trigger-consistent",
                                 "P7-positive-clause-repeat-consistent",
                                 "P8-positive-quoted-trigger-wordname",
                                 "P9-positive-quoted-wordname-corner",
                                 "P10-positive-latest-synonym-consistent",
                                 "P11-positive-quoted-consistent-mention") else "PASS-NEG"
        print(f"{tag if passed else 'FAIL'} {name}: {detail}")
    print(f"SELF-TEST {'OK' if all_ok else 'FAILED'} ({sum(1 for x in results if x[1])}/{len(results)})")
    return 0 if all_ok else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--inputs", default=str(DEFAULT_INPUTS))
    ap.add_argument("--register", default=str(DEFAULT_REGISTER))
    ap.add_argument("--out", default=None)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        sys.exit(self_test())

    verdict, report = evaluate(_load(args.inputs), _load(args.register))
    report["inputs_file"] = args.inputs
    report["register_file"] = args.register
    if args.out:
        Path(args.out).write_text(json.dumps(report, ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    for x in report["contract_violations"]:
        print(f"CONTRACT-VIOLATION: {x}")
    for x in report["register_violations"]:
        print(f"REGISTER-VIOLATION: {x}")
    for name, a in report["areas"].items():
        print(f"[{a['verdict']:>12}] {name}")
        for c in a["capabilities"]:
            print(f"    {c['id']}: {c['status']} -> {c['result']}")
        for p in a["problems"]:
            print(f"    PROBLEM: {p}")
    print(f"VERDICT: {verdict}")
    print(f"SHELL-REPLACEMENT: {report['shell_replacement_path']}")
    sys.exit(0 if verdict in ("PASS", "PASS_WITH_CONDITIONS") else 1)


if __name__ == "__main__":
    main()
