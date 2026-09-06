/**
 * semantic-input-provenance.ts — Semantic Input Provenance 契约（Phase 5）。
 *
 * 定位：描述一次 Model Call 的 **业务语义输入**（systemPrompt / messages / tools /
 * media prompt + reference / audio + language）每一部分的
 *
 *   - category  它是什么（有限枚举，与 subsystem/operation/callPurpose 严格正交）
 *   - role      它以什么对话角色出现（system/user/assistant/tool/input/parameter）
 *   - source    它从哪里来（类型 + 安全逻辑 id + 可选版本；禁止绝对路径/URL/凭证）
 *   - locator   它在 Semantic Request 的哪个位置（root + path + 可选 UTF-16 span）
 *   - precision 这条记录精确到什么程度（exact / structural / opaque）
 *
 * 红线（§五/§三十七）：provenance **不包含任何输入内容**。section 只有上述五个
 * 维度；没有任何字段承载 prompt/message/tool result/audio 的正文。未来 Phase 6
 * 的 Request Capture 拿到真实 Semantic Request 后，用 locator 解析内容，
 * provenance 单独存在时无法还原任何正文。
 *
 * Span 语义（§二十七）：JavaScript String 的 UTF-16 code unit 偏移，闭开区间
 * [start, end)，即 `text.slice(start, end) === sectionText`。代理对（emoji 等）
 * 占 2 个 code unit，与 String.prototype.slice 语义一致。
 *
 * Precision 语义（§二十八～§三十二）：
 *   exact       来源已知 + 位置由运行时实际对象证明（非模板重建）
 *   structural  位置已知（或范围已知）但来源只能粗分类；identity-only 段
 *               （span=null）也属于 structural——知道是什么，无法定位
 *   opaque      知道 SDK/Adapter 在该位置加入了输入，但 Lingxi 无法定位内容
 *
 * Call 级 rollup（§一百一十四）：
 *   exact    全部 meaningful sections 均 exact
 *   partial  存在 exact 且存在 structural/opaque；或仅部分根可见
 *   opaque   主要语义输入完全在外部不可见（全部 sections 为 opaque）
 * 禁止百分比（§一百一十五）。
 */

/* ── 闭集与 wire 类型：已迁移到 shared/model-observability-api-contract.ts ──
 *
 * Phase 9（§九）：renderer 的 Provenance Inspector 需要消费 category/role/
 * source/root/shape 闭集与 section/locator/span wire 形状，但本文件链路在
 * server 侧。全部枚举数组与纯 JSON wire 类型收拢到 shared 单一事实源，此处
 * re-export 保持全部既有 import 站点（observer/capture/builder/tests）不变；
 * sanitize/rollup/summarize/renderer 等 server 逻辑留在本文件。
 */
export {
  SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION,
  SEMANTIC_INPUT_CATEGORIES,
  SEMANTIC_INPUT_ROLES,
  SEMANTIC_SOURCE_TYPES,
  SEMANTIC_INPUT_ROOTS,
  SEMANTIC_INPUT_SHAPES,
  MAX_PROVENANCE_SECTIONS,
} from "../../shared/model-observability-api-contract.ts";
export type {
  SemanticInputCategory,
  SemanticInputRole,
  SemanticSourceType,
  SemanticInputRoot,
  SemanticInputShape,
  SemanticInputSpan,
  SemanticInputLocator,
  SemanticInputSource,
  SemanticInputProvenanceSection,
  SemanticInputProvenancePrecision,
  ModelSemanticInputProvenance,
} from "../../shared/model-observability-api-contract.ts";
import {
  SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION,
  SEMANTIC_INPUT_CATEGORIES,
  SEMANTIC_INPUT_ROLES,
  SEMANTIC_SOURCE_TYPES,
  SEMANTIC_INPUT_ROOTS,
  SEMANTIC_INPUT_SHAPES,
  MAX_PROVENANCE_SECTIONS,
} from "../../shared/model-observability-api-contract.ts";
import type {
  SemanticInputCategory,
  SemanticInputRole,
  SemanticSourceType,
  SemanticInputRoot,
  SemanticInputShape,
  SemanticInputSpan,
  SemanticInputLocator,
  SemanticInputSource,
  SemanticInputProvenanceSection,
  SemanticInputProvenancePrecision,
  ModelSemanticInputProvenance,
} from "../../shared/model-observability-api-contract.ts";

/* ── Sanitize gate（fail closed，§三十七机器防线）────────────────────── */

const CATEGORY_SET = new Set<string>(SEMANTIC_INPUT_CATEGORIES);
const ROLE_SET = new Set<string>(SEMANTIC_INPUT_ROLES);
const SOURCE_TYPE_SET = new Set<string>(SEMANTIC_SOURCE_TYPES);
const ROOT_SET = new Set<string>(SEMANTIC_INPUT_ROOTS);
const SHAPE_SET = new Set<string>(SEMANTIC_INPUT_SHAPES);

const SOURCE_ID_MAX_CHARS = 128;
const SOURCE_VERSION_MAX_CHARS = 64;
const PATH_ITEM_STRING_MAX_CHARS = 64;
const PATH_MAX_ITEMS = 16;

function safeBoundedString(value: unknown, max: number): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > max) return null;
  // 禁止绝对路径 / UNC / drive letter / URL 形态（§二十二）。
  if (/^([a-zA-Z]:[\\/]|\/|\\\\)/.test(value)) return null;
  if (value.includes("://")) return null;
  return value;
}

function sanitizeSource(source: unknown): SemanticInputSource | null {
  if (!source || typeof source !== "object" || Array.isArray(source)) return null;
  const record = source as Record<string, unknown>;
  const type = record.type;
  if (typeof type !== "string" || !SOURCE_TYPE_SET.has(type)) return null;
  const out: SemanticInputSource = { type: type as SemanticSourceType };
  if (record.id !== undefined && record.id !== null) {
    const id = safeBoundedString(record.id, SOURCE_ID_MAX_CHARS);
    if (id === null) return null; // 非法 id → 整个 source 丢弃（fail closed）
    out.id = id;
  }
  if (record.version !== undefined && record.version !== null) {
    const version = safeBoundedString(record.version, SOURCE_VERSION_MAX_CHARS);
    if (version === null) return null;
    out.version = version;
  }
  return out;
}

const SPAN_INVALID = Symbol("span-invalid");

function sanitizeSpan(span: unknown): SemanticInputSpan | null | undefined | typeof SPAN_INVALID {
  if (span === null) return null; // identity-only（structural/opaque 专用）
  if (span === undefined) return undefined; // 非文本根的 index/key 寻址
  if (!span || typeof span !== "object") return SPAN_INVALID;
  const record = span as Record<string, unknown>;
  const start = record.start;
  const end = record.end;
  if (typeof start !== "number" || typeof end !== "number") return SPAN_INVALID;
  if (!Number.isInteger(start) || !Number.isInteger(end)) return SPAN_INVALID;
  if (start < 0 || end < start) return SPAN_INVALID;
  return { start, end };
}

function sanitizeLocator(locator: unknown): SemanticInputLocator | null {
  if (!locator || typeof locator !== "object" || Array.isArray(locator)) return null;
  const record = locator as Record<string, unknown>;
  if (typeof record.root !== "string" || !ROOT_SET.has(record.root)) return null;
  const out: SemanticInputLocator = { root: record.root as SemanticInputRoot };
  if (record.path !== undefined && record.path !== null) {
    if (!Array.isArray(record.path) || record.path.length > PATH_MAX_ITEMS) return null;
    const path: Array<number | string> = [];
    for (const item of record.path) {
      if (typeof item === "number" && Number.isInteger(item) && item >= 0) {
        path.push(item);
      } else if (typeof item === "string" && item.length > 0
        && item.length <= PATH_ITEM_STRING_MAX_CHARS) {
        path.push(item);
      } else {
        return null;
      }
    }
    out.path = path;
  }
  const span = sanitizeSpan(record.span);
  if (span === SPAN_INVALID) return null; // 非法 span → 整段丢弃（fail closed）
  if (span !== undefined) out.span = span;
  return out;
}

export function sanitizeSemanticInputSection(section: unknown): SemanticInputProvenanceSection | null {
  if (!section || typeof section !== "object" || Array.isArray(section)) return null;
  const record = section as Record<string, unknown>;
  if (typeof record.category !== "string" || !CATEGORY_SET.has(record.category)) return null;
  const precision = record.precision;
  if (precision !== "exact" && precision !== "structural" && precision !== "opaque") return null;
  const locator = sanitizeLocator(record.locator);
  if (!locator) return null;
  // exact 不允许 span 缺失的 identity-only 段（identity-only 必须 structural/opaque）。
  if (precision === "exact" && locator.span === null) return null;
  const source = record.source === undefined || record.source === null
    ? null
    : sanitizeSource(record.source);
  if (record.source !== undefined && record.source !== null && !source) return null;
  let role: SemanticInputRole | null = null;
  if (record.role !== undefined && record.role !== null) {
    if (typeof record.role !== "string" || !ROLE_SET.has(record.role)) return null;
    role = record.role as SemanticInputRole;
  }
  // 统一形状：role/source 未提供时显式 null（序列化形状稳定）。
  return {
    category: record.category as SemanticInputCategory,
    role,
    precision,
    locator,
    source,
  };
}

/**
 * 整包 sanitize：非法 section 逐条丢弃（fail closed），全部非法/输入非对象 → null。
 * 超过 MAX_PROVENANCE_SECTIONS 的尾段折叠为单条 structural 段（记录折叠起点）。
 */
export function sanitizeSemanticInputProvenance(input: unknown): ModelSemanticInputProvenance | null {
  if (!input || typeof input !== "object" || Array.isArray(input)) return null;
  const record = input as Record<string, unknown>;
  if (record.schemaVersion !== SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION) return null;
  if (typeof record.inputShape !== "string" || !SHAPE_SET.has(record.inputShape)) return null;
  if (!Array.isArray(record.sections)) return null;
  const sections: SemanticInputProvenanceSection[] = [];
  let firstDroppedIndex: number | null = null;
  for (let i = 0; i < record.sections.length; i++) {
    const safe = sanitizeSemanticInputSection(record.sections[i]);
    if (!safe) continue;
    if (sections.length >= MAX_PROVENANCE_SECTIONS) {
      if (firstDroppedIndex === null) firstDroppedIndex = i;
      continue;
    }
    sections.push(safe);
  }
  if (firstDroppedIndex !== null) {
    sections.push({
      category: "unknown",
      precision: "structural",
      locator: { root: "messages", path: [firstDroppedIndex], span: null },
      source: { type: "unknown", id: "provenance-section-overflow" },
    });
  }
  if (sections.length === 0) return null;
  return {
    schemaVersion: SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION,
    inputShape: record.inputShape as SemanticInputShape,
    sections,
  };
}

/* ── Precision rollup + Observer summary（§一百一十四/§三十九）───────── */

export function rollupProvenancePrecision(
  provenance: ModelSemanticInputProvenance | null,
): SemanticInputProvenancePrecision | null {
  if (!provenance || provenance.sections.length === 0) return null;
  let exact = 0;
  let structural = 0;
  let opaque = 0;
  for (const section of provenance.sections) {
    if (section.precision === "exact") exact += 1;
    else if (section.precision === "structural") structural += 1;
    else opaque += 1;
  }
  if (opaque > 0 && exact + structural === 0) return "opaque";
  if (structural + opaque > 0) return "partial";
  return "exact";
}

/**
 * Observer 事件 details 的安全 summary（§三十九/§七十六）：只放 precision/
 * 计数/去重 categories。完整 section map 不进事件——经 recorder 的 symbol
 * 引用（MODEL_CALL_SEMANTIC_PROVENANCE）供测试/未来 Request Capture 按 callId
 * 关联，不参与 JSON 序列化。
 */
export function summarizeSemanticInputProvenance(
  provenance: ModelSemanticInputProvenance | null,
): Record<string, unknown> | null {
  if (!provenance) return null;
  const categories: string[] = [];
  let opaqueSections = 0;
  for (const section of provenance.sections) {
    if (!categories.includes(section.category)) categories.push(section.category);
    if (section.precision === "opaque") opaqueSections += 1;
  }
  return {
    inputShape: provenance.inputShape,
    provenancePrecision: rollupProvenancePrecision(provenance),
    inputSectionCount: provenance.sections.length,
    inputCategories: categories.slice(0, 32),
    opaqueSectionCount: opaqueSections,
  };
}

/* ── Provenanced Text Renderer（§三十三/§三十四）────────────────────────
 *
 * 唯一保证：rendered text 与调用方原先的 `parts.join(separator)` 字节级一致
 * （UTF-16 语义）。renderer 不 filter、不 trim、不补分隔符——空 section 也
 * 参与拼接（["a","","b"].join(sep) 语义保持），零宽 span 合法。
 */

export type ProvenancedTextSegment = {
  text: string;
  category: SemanticInputCategory;
  role?: SemanticInputRole | null;
  source?: SemanticInputSource | null;
};

/** 类型化 segment 构造（闭集 category/source 经参数类型收窄，供调用方避免 widen）。 */
export function provenancedSegment(
  text: string,
  category: SemanticInputCategory,
  source?: { type?: SemanticSourceType; id?: string } | null,
): ProvenancedTextSegment {
  return {
    text,
    category,
    source: source
      ? { type: source.type ?? "runtime", ...(source.id ? { id: source.id } : {}) }
      : null,
  };
}

export type RenderedProvenancedText = {
  text: string;
  sections: SemanticInputProvenanceSection[];
};

export function renderProvenancedText(
  segments: readonly ProvenancedTextSegment[],
  separator: string,
  locator: { root: SemanticInputRoot; path?: Array<number | string> } = { root: "systemPrompt" },
): RenderedProvenancedText {
  const texts = segments.map((segment) =>
    typeof (segment as ProvenancedTextSegment)?.text === "string" ? segment.text : "");
  const text = texts.join(separator);
  const sections: SemanticInputProvenanceSection[] = [];
  let offset = 0;
  for (let i = 0; i < texts.length; i++) {
    const segment = segments[i] as ProvenancedTextSegment;
    const length = texts[i].length;
    if (length > 0 || texts.length === 1) {
      // 零宽段只在「唯一段」时记录（多段拼接中的空段是排版噪音，不产生 section）。
      sections.push({
        category: segment.category,
        role: segment.role ?? null,
        precision: "exact",
        locator: {
          root: locator.root,
          ...(locator.path ? { path: [...locator.path] } : {}),
          span: { start: offset, end: offset + length },
        },
        source: segment.source ?? null,
      });
    }
    offset += length;
    if (i < texts.length - 1) offset += separator.length;
  }
  return { text, sections };
}

/* ── 构造 helper ──────────────────────────────────────────────────────── */

export function createSemanticInputProvenance(
  inputShape: SemanticInputShape,
  sections: readonly SemanticInputProvenanceSection[],
): ModelSemanticInputProvenance | null {
  return sanitizeSemanticInputProvenance({
    schemaVersion: SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION,
    inputShape,
    sections,
  });
}

/** 小 helper：默认 exact（要求 locator 自带可用 span；identity-only 段显式传 precision=structural 并 span=null）。 */
export function provenanceSection(
  locator: SemanticInputLocator,
  category: SemanticInputCategory,
  options: {
    role?: SemanticInputRole | null;
    precision?: "exact" | "structural" | "opaque";
    source?: SemanticInputSource | null;
  } = {},
): SemanticInputProvenanceSection {
  return {
    category,
    role: options.role ?? null,
    precision: options.precision ?? "exact",
    locator,
    source: options.source ?? null,
  };
}

/**
 * Observer 事件上的 provenance 引用（non-enumerable symbol）：完整 section map
 * 不进事件 details（§三十九），但以引用形态挂在事件对象上，供测试 collector
 * 与未来 Request Capture 在同一 callId 下取得「Semantic Request + Provenance」。
 * symbol 键不参与 JSON.stringify / Metadata Safety Gate。
 */
export const MODEL_CALL_SEMANTIC_PROVENANCE: unique symbol = Symbol.for(
  "lingxi.modelCallSemanticInputProvenance",
);

/* ── MC-01/03：Pi streamFn chat context 构造器 ─────────────────────────── */

/**
 * Session 冻结快照携带的 prompt provenance（安全 metadata；不含内容副本）。
 * `sections` 的 span 相对 customPrompt（root=systemPrompt，起点 0）。
 */
export type SessionPromptProvenancePayload = {
  customPrompt: string;
  sections: SemanticInputProvenanceSection[];
  appendSystemPrompt?: string[] | null;
  skillNames?: string[] | null;
  agentsFileNames?: string[] | null;
};

function safeNameList(values: unknown, max: number): string[] {
  if (!Array.isArray(values)) return [];
  const out: string[] = [];
  for (const value of values) {
    if (typeof value === "string" && value.trim() && value.length <= 64) out.push(value.trim());
    if (out.length >= max) break;
  }
  return out;
}

function identityOnlySection(
  category: SemanticInputCategory,
  source: SemanticInputSource,
): SemanticInputProvenanceSection {
  return {
    category,
    role: null,
    precision: "structural",
    locator: { root: "systemPrompt", span: null },
    source,
  };
}

/**
 * MC-01（普通 chat）/ MC-03（native summarizer）在 streamFunction 边界的
 * provenance 构造。原审计依据见 docs/archives/model-observability/SEMANTIC_INPUT_PROVENANCE_AUDIT.md §2/§4：
 *
 *   systemPrompt:
 *     - native summarization → 单段 structural task_instruction（SDK 模板镜像
 *       非同源数据结构，runtime 等值不作 exact 依据，§三十二）。
 *     - 有冻结快照且 runtime 前缀验证通过（startsWith(customPrompt)，对真实
 *       冻结对象验证而非模板重建）→ 快照 sections 原样成立（exact，平移 0）；
 *       SDK 尾部（append+project_context+skills+cwd 混合）→ 单段 structural
 *       sdk_internal；append/skills/agentsFiles 以 identity-only 段记录。
 *     - 无快照/验证失败 → 单段 structural session_instruction（诚实降级，
 *       §八十五：旧 session 不伪造 FULL）。
 *   messages: role=toolResult → tool_result（exact）；role=user 且处于 prompt
 *     turn 且为 turn 内最后一条 user → current_user_input（runtime turn 不变量
 *     证明：loop 只追加 assistant/toolResult，toolResult 是独立 role；§五十一
 *     禁止的「数组最后一项」启发式不成立时宁可 conversation_history）；
 *     其余 user/assistant → conversation_history。
 *   tools: 每项 tool_definition（source=tool name，不存 schema）。
 */
export function buildChatContextProvenance(
  context: { systemPrompt?: unknown; messages?: unknown; tools?: unknown },
  options: {
    promptTurn?: boolean;
    promptSnapshot?: SessionPromptProvenancePayload | null;
    nativeSummarization?: boolean;
  } = {},
): ModelSemanticInputProvenance | null {
  const sections: SemanticInputProvenanceSection[] = [];
  const systemPrompt = typeof context.systemPrompt === "string" ? context.systemPrompt : null;

  if (systemPrompt !== null) {
    if (options.nativeSummarization) {
      sections.push(provenanceSection(
        { root: "systemPrompt", span: { start: 0, end: systemPrompt.length } },
        "task_instruction",
        {
          precision: "structural",
          role: "system",
          source: { type: "sdk", id: "pi.native-summarizer.system" },
        },
      ));
    } else {
      const snapshot = options.promptSnapshot ?? null;
      const prefixVerified = snapshot !== null
        && snapshot.customPrompt.length > 0
        && systemPrompt.startsWith(snapshot.customPrompt);
      if (prefixVerified) {
        for (const section of snapshot.sections) {
          if (section.locator.root === "systemPrompt") sections.push(section);
        }
        if (systemPrompt.length > snapshot.customPrompt.length) {
          sections.push(provenanceSection(
            {
              root: "systemPrompt",
              span: { start: snapshot.customPrompt.length, end: systemPrompt.length },
            },
            "sdk_internal",
            {
              precision: "structural",
              role: "system",
              source: { type: "sdk", id: "pi.system-prompt-suffix" },
            },
          ));
        }
        if (snapshot.appendSystemPrompt && snapshot.appendSystemPrompt.length > 0) {
          sections.push(identityOnlySection(
            "session_instruction",
            { type: "runtime", id: "session.appendSystemPrompt" },
          ));
        }
        const skillNames = safeNameList(snapshot.skillNames, 8);
        if (skillNames.length > 0) {
          sections.push(identityOnlySection(
            "skill_instruction",
            { type: "skill", id: skillNames.join(",") },
          ));
        }
        const agentsFileNames = safeNameList(snapshot.agentsFileNames, 8);
        if (agentsFileNames.length > 0) {
          sections.push(identityOnlySection(
            "agents_file",
            { type: "runtime", id: agentsFileNames.join(",") },
          ));
        }
      } else if (systemPrompt.length > 0) {
        sections.push(provenanceSection(
          { root: "systemPrompt", span: { start: 0, end: systemPrompt.length } },
          "session_instruction",
          {
            precision: "structural",
            role: "system",
            source: { type: "snapshot", id: "session.systemPrompt" },
          },
        ));
      }
    }
  }

  if (Array.isArray(context.messages)) {
    const messages = context.messages;
    let lastUserIndex = -1;
    if (!options.nativeSummarization) {
      for (let i = messages.length - 1; i >= 0; i--) {
        if ((messages[i] as any)?.role === "user") { lastUserIndex = i; break; }
      }
    }
    for (let i = 0; i < messages.length && sections.length < MAX_PROVENANCE_SECTIONS; i++) {
      const message = messages[i] as any;
      const role = typeof message?.role === "string" ? message.role : null;
      const locator: SemanticInputLocator = { root: "messages", path: [i] };
      if (options.nativeSummarization) {
        // SDK serializeConversation 拍平后的任务输入：位置可知（path=[i]），
        // 内部组成（conversation/previous-summary 追加）不可拆 → structural。
        sections.push(provenanceSection(
          locator,
          "task_input",
          {
            precision: "structural",
            role: "user",
            source: { type: "sdk", id: "pi.serializeConversation" },
          },
        ));
        continue;
      }
      if (role === "toolResult") {
        const toolName = typeof message.toolName === "string" && message.toolName.trim()
          ? message.toolName.trim()
          : null;
        sections.push(provenanceSection(
          locator,
          "tool_result",
          {
            role: "tool",
            source: { type: "tool", ...(toolName ? { id: toolName } : {}) },
          },
        ));
      } else if (role === "user") {
        const isCurrentInput = options.promptTurn === true && i === lastUserIndex;
        sections.push(provenanceSection(
          locator,
          isCurrentInput ? "current_user_input" : "conversation_history",
          { role: "user" },
        ));
      } else if (role === "assistant") {
        sections.push(provenanceSection(locator, "conversation_history", { role: "assistant" }));
      } else if (role === "system" || role === "developer") {
        sections.push(provenanceSection(locator, "session_instruction", { role: "system" }));
      } else {
        sections.push(provenanceSection(
          locator,
          "conversation_history",
          { precision: "structural", role: null },
        ));
      }
    }
  }

  if (Array.isArray(context.tools)) {
    for (let i = 0; i < context.tools.length && sections.length < MAX_PROVENANCE_SECTIONS; i++) {
      const tool = context.tools[i] as any;
      const name = typeof tool?.name === "string" && tool.name.trim() ? tool.name.trim() : null;
      sections.push(provenanceSection(
        { root: "tools", path: [i] },
        "tool_definition",
        { source: { type: "tool", ...(name ? { id: name } : {}) } },
      ));
    }
  }

  return createSemanticInputProvenance("chat_context", sections);
}

/* ── MC-02：scope provenance 的尾段扩展 ───────────────────────────────── */

/**
 * MC-02 runner 冻结了发起时的消息覆盖（liveMessages + instruction），但
 * recovery/repair 的后续 streamFn 调用里 SDK loop 已追加 assistant/toolResult。
 * 本函数按同一 role 分类规则补齐**未被覆盖的尾部消息**（不能证明 turn 归属，
 * user 一律 conversation_history，§五十一）。已覆盖前缀原样保留。
 */
export function extendChatContextProvenance(
  provenance: ModelSemanticInputProvenance | null,
  messages: readonly unknown[],
): ModelSemanticInputProvenance | null {
  if (!provenance) return null;
  if (!Array.isArray(messages) || messages.length === 0) return provenance;
  let coveredCount = 0;
  let sawMessageSections = false;
  for (const section of provenance.sections) {
    if (section.locator.root !== "messages") continue;
    const first = section.locator.path?.[0];
    if (typeof first === "number") {
      sawMessageSections = true;
      coveredCount = Math.max(coveredCount, first + 1);
    }
  }
  if (!sawMessageSections) coveredCount = 0;
  const extra: SemanticInputProvenanceSection[] = [];
  for (let i = coveredCount; i < messages.length && extra.length < MAX_PROVENANCE_SECTIONS; i++) {
    const message = messages[i] as any;
    const role = typeof message?.role === "string" ? message.role : null;
    const locator: SemanticInputLocator = { root: "messages", path: [i] };
    if (role === "toolResult") {
      const toolName = typeof message.toolName === "string" && message.toolName.trim()
        ? message.toolName.trim()
        : null;
      extra.push(provenanceSection(
        locator,
        "tool_result",
        { role: "tool", source: { type: "tool", ...(toolName ? { id: toolName } : {}) } },
      ));
    } else if (role === "user" || role === "assistant") {
      extra.push(provenanceSection(
        locator,
        "conversation_history",
        { role: role as SemanticInputRole },
      ));
    } else {
      extra.push(provenanceSection(
        locator,
        "conversation_history",
        { precision: "structural", role: null },
      ));
    }
  }
  if (extra.length === 0) return provenance;
  return createSemanticInputProvenance(provenance.inputShape, [...provenance.sections, ...extra]);
}

/* ── MC-04：callText 归一化（§五十九/§六十）──────────────────────────── */

export type CallTextProvenanceMessage = { role?: unknown; content?: unknown };

/**
 * callText Semantic Request Boundary 的 provenance 归一化：
 * 调用方按**传入形状**（root=systemPrompt + root=messages path=[原始 index]）
 * 描述；本函数随 system 消息 merge（llm-client.ts:509-522 的同一变换）同步
 * remap——system 消息 span 平移进 merged system；非 system 消息 index 重排。
 * caller 未提供 provenance 时用 buildCallTextFallbackProvenance。
 */
export function remapCallTextProvenance(
  provenance: ModelSemanticInputProvenance | null,
  args: {
    systemPrompt: string;
    messages: readonly CallTextProvenanceMessage[];
    systemTextOf: (message: CallTextProvenanceMessage) => string;
  },
): ModelSemanticInputProvenance | null {
  if (!provenance) return null;
  // 与 llm-client merge 逻辑逐行同构：mergedSystem += (mergedSystem ? "\n" : "") + text
  const systemSpanFor = new Map<number, { start: number; end: number }>();
  let mergedLength = args.systemPrompt.length;
  for (let i = 0; i < args.messages.length; i++) {
    const message = args.messages[i];
    if (message?.role !== "system") continue;
    const text = args.systemTextOf(message);
    if (!text) continue;
    const start = mergedLength > 0 ? mergedLength + 1 : 0;
    systemSpanFor.set(i, { start, end: start + text.length });
    mergedLength = start + text.length;
  }
  let normalizedIndex = 0;
  const normalizedIndexFor = new Map<number, number>();
  for (let i = 0; i < args.messages.length; i++) {
    if (args.messages[i]?.role === "system") continue;
    normalizedIndexFor.set(i, normalizedIndex);
    normalizedIndex += 1;
  }

  const sections: SemanticInputProvenanceSection[] = [];
  for (const section of provenance.sections) {
    const locator = section.locator;
    if (locator.root === "systemPrompt") {
      sections.push(section);
      continue;
    }
    if (locator.root === "messages" && Array.isArray(locator.path) && locator.path.length >= 1) {
      const index = locator.path[0];
      if (typeof index !== "number" || index < 0 || index >= args.messages.length) continue;
      if (args.messages[index]?.role === "system") {
        const span = systemSpanFor.get(index);
        if (!span) continue; // 空文本 system 消息被 merge 跳过，段随之消失
        const sectionSpan = locator.span ?? null;
        const nextSpan = sectionSpan
          ? { start: span.start + sectionSpan.start, end: span.start + sectionSpan.end }
          : span;
        sections.push({
          ...section,
          locator: { root: "systemPrompt", path: undefined, span: nextSpan },
        });
        continue;
      }
      const nextIndex = normalizedIndexFor.get(index);
      if (nextIndex === undefined) continue;
      sections.push({
        ...section,
        locator: { root: "messages", path: [nextIndex], span: locator.span ?? undefined },
      });
      continue;
    }
    sections.push(section);
  }
  return createSemanticInputProvenance("calltext", sections);
}

/**
 * §六十一 fallback：caller 未显式提供 provenance 时——
 * systemPrompt（含 system 消息，merge 前）→ task_instruction structural；
 * 非_system 消息 → task_input structural。fallback 一律不得 exact。
 */
export function buildCallTextFallbackProvenance(args: {
  systemPrompt: string;
  messages: readonly CallTextProvenanceMessage[];
}): ModelSemanticInputProvenance | null {
  const sections: SemanticInputProvenanceSection[] = [];
  if (args.systemPrompt.length > 0) {
    sections.push(provenanceSection(
      { root: "systemPrompt", span: { start: 0, end: args.systemPrompt.length } },
      "task_instruction",
      { precision: "structural", role: "system", source: { type: "runtime" } },
    ));
  }
  for (let i = 0; i < args.messages.length; i++) {
    const message = args.messages[i];
    if (message?.role === "system") {
      sections.push(provenanceSection(
        { root: "messages", path: [i] },
        "task_instruction",
        { precision: "structural", role: "system", source: { type: "runtime" } },
      ));
    } else {
      sections.push(provenanceSection(
        { root: "messages", path: [i] },
        "task_input",
        { precision: "structural", role: null },
      ));
    }
  }
  return createSemanticInputProvenance("calltext", sections);
}

/* ── MC-10：Pi direct summary 参数形状（§七十）───────────────────────── */

export function buildDirectSummaryProvenance(args: {
  messages: readonly unknown[];
  customInstructions?: string | null;
  previousSummary?: string | null;
}): ModelSemanticInputProvenance | null {
  const sections: SemanticInputProvenanceSection[] = [];
  for (let i = 0; i < args.messages.length && sections.length < MAX_PROVENANCE_SECTIONS; i++) {
    const message = args.messages[i] as any;
    const role = typeof message?.role === "string" ? message.role : null;
    const locator: SemanticInputLocator = { root: "messages", path: [i] };
    if (role === "user") {
      sections.push(provenanceSection(locator, "conversation_history", { role: "user" }));
    } else if (role === "assistant") {
      sections.push(provenanceSection(locator, "conversation_history", { role: "assistant" }));
    } else if (role === "toolResult") {
      const toolName = typeof message?.toolName === "string" && message.toolName.trim()
        ? message.toolName.trim()
        : null;
      sections.push(provenanceSection(
        locator,
        "tool_result",
        { role: "tool", source: { type: "tool", ...(toolName ? { id: toolName } : {}) } },
      ));
    } else {
      sections.push(provenanceSection(
        locator,
        "conversation_history",
        { precision: "structural", role: null },
      ));
    }
  }
  if (typeof args.customInstructions === "string" && args.customInstructions.length > 0) {
    sections.push(provenanceSection(
      { root: "parameters", path: ["customInstructions"] },
      "task_instruction",
      { role: "system", source: { type: "runtime", id: "diary.temporary-summary.instructions" } },
    ));
  }
  if (typeof args.previousSummary === "string" && args.previousSummary.length > 0) {
    sections.push(provenanceSection(
      { root: "parameters", path: ["previousSummary"] },
      "previous_summary",
      { role: "system", source: { type: "memory" } },
    ));
  }
  return createSemanticInputProvenance("pi_direct_summary", sections);
}
