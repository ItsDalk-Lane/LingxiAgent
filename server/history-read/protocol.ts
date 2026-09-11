/**
 * E01/E02：历史分页条件 GET（协议 v1）。
 *
 * 接入点 = 普通消息路由（/sessions/messages，非 all/reconciliation）的**成功返回
 * 边界**：授权、B/C 页面读取与快照复核、外部状态补齐、lifecycle/rebroadcast 全部
 * 完成之后（E02.1 八步流程的第 5–8 步）。本模块不做任何读取，不打开
 * SessionManager，不遍历分支——分支选择身份直接取自本次 B/C 目录上下文。
 *
 * 标签为不透明弱 ETag `W/"hrp1-<sha256>"`，摘要输入 = 协议/序列化版本 + 进程内
 * 随机盐 + 作用域身份（主体/runtime/studio/会话/规范 locator，全部非敏感内部身份）
 * + 规范化请求身份（端点、before 语义、实际 limit、模式、语言维度）+ 分支选择身份
 * + 响应 UTF-8 字节。盐不落盘：服务重启造成旧标签失配 = 正常 200 回退。
 *
 * 分支选择身份只含 selectedLeafId（决定了页面展示哪条分支）：headResolution 是
 * 选择机制（legacy_tail/persisted_head 的首次写回过渡）而非表示维度；physical
 * tail 变化（弃支追加）不改变当前页表示——两者进摘要只会让标签在表示未变时
 * 失配（首次请求→head 写回→第二次请求标签必失配，E1 失去意义）。
 *
 * 身份不可靠、revision=null、普通读取降级（legacy 全量兜底）时不签发可复用标签。
 * 日志只含 requestId/结果/reason/截短不可逆摘要（E02.3）。
 */
import { createHash, randomBytes } from "node:crypto";
import { createModuleLogger } from "../../lib/debug-log.ts";

const log = createModuleLogger("sessions/history-read");

export const HISTORY_PROTOCOL_VERSION = 1;
export const HISTORY_PROTOCOL_TAG_PREFIX = "hrp1";

/**
 * E02/E06.2 单一配置点：`Lingxi-History-Page-Limit` 声明值 = 推荐页大小 K。
 * E06.2 决策（page-size-decision.md）：K=100——主要收益（10k 受控链路全翻往返
 * 201→101 次、总时 31.8s→16.1s）在 K=100 已取得，K≥150 首屏延迟上升明显且
 * 单页载荷翻倍，边际收益不抵成本。省略 limit 的旧请求仍由服务端默认 50 承接
 *（声明值是给新客户端的建议，不改变服务端默认），显式 limit 上限 200 不变。
 */
export const HISTORY_PROTOCOL_PAGE_LIMIT = 100;

/** 进程内随机作用域盐：不落盘、不改持久化 schema；重启即换（旧标签正常失配）。 */
const scopeSalt = randomBytes(32).toString("hex");

export interface HistoryBranchIdentity {
  selectedLeafId: string | null;
  physicalTailLeafId: string | null;
  headResolution: string | null;
}

export interface HistoryPageTagInput {
  protocolVersion: number;
  scope: {
    principalId: string | null;
    serverNodeId: string | null;
    studioId: string | null;
    sessionId: string | null;
    normalizedPath: string | null;
    branchIdentity: HistoryBranchIdentity | null;
  };
  endpoint: string;
  before: number | null; // null = 最新页
  limit: number;
  mode: string; // "normal"（all/reconciliation 不进入本函数）
  language: string; // 当前响应无语言变体，恒 ""（表示维度预留）
  revision: string | null;
  responseBodyUtf8: string;
}

/**
 * 由当前响应与作用域生成可复用标签；不可签发时返回 null（该响应按普通 200 处理，
 * 不携带 ETag/协议头）。
 */
export function buildHistoryPageTag(input: HistoryPageTagInput): string | null {
  // revision=null / 身份不可靠（无规范路径）→ 不签发（E02.2/E02.3）。
  if (!input.revision) return null;
  if (!input.scope.normalizedPath) return null;
  if (input.protocolVersion !== HISTORY_PROTOCOL_VERSION) return null;
  const parts = [
    `${HISTORY_PROTOCOL_TAG_PREFIX}:v${input.protocolVersion}`,
    scopeSalt,
    input.scope.principalId ?? "",
    input.scope.serverNodeId ?? "",
    input.scope.studioId ?? "",
    input.scope.sessionId ?? "",
    input.scope.normalizedPath,
    input.endpoint,
    input.before == null ? "latest" : String(input.before),
    String(input.limit),
    input.mode,
    input.language,
    input.scope.branchIdentity?.selectedLeafId ?? "",
    input.revision,
    input.responseBodyUtf8,
  ];
  const digest = createHash("sha256").update(parts.join("\u0000"), "utf8").digest("hex");
  return `W/"${HISTORY_PROTOCOL_TAG_PREFIX}-${digest}"`;
}

/**
 * RFC 9110 If-None-Match（GET 场景使用弱比较）：忽略 W/ 前缀比较不透明部分。
 * `*` = 资源存在即匹配（调用方已先授权并成功读取，资源存在性已确认）。
 * 语法无效（无任何合法实体标签元素）→ "invalid"：调用方忽略条件头正常 200。
 * 官方客户端只回显服务器下发的单个标签；`*` 面向外部合法请求。
 */
export function evaluateIfNoneMatch(
  headerValue: string | undefined | null,
  currentTag: string,
): "match" | "no-match" | "invalid" {
  if (typeof headerValue !== "string") return "invalid";
  const raw = headerValue.trim();
  if (raw === "") return "invalid";
  if (raw === "*") return "match";
  const currentOpaque = currentTag.startsWith("W/") ? currentTag.slice(2).trim() : currentTag.trim();
  const currentInner = currentOpaque.length >= 2 && currentOpaque.startsWith('"') && currentOpaque.endsWith('"')
    ? currentOpaque.slice(1, -1)
    : currentOpaque;
  let sawValidElement = false;
  for (const element of raw.split(",")) {
    const value = element.trim();
    if (value === "") continue;
    let opaque = value;
    if (opaque.startsWith("W/") || opaque.startsWith("w/")) opaque = opaque.slice(2).trim();
    if (opaque.length < 2 || !opaque.startsWith('"') || !opaque.endsWith('"')) continue;
    const inner = opaque.slice(1, -1);
    if (/[\r\n\\]/.test(inner)) continue; // 非法字符：该元素无效
    sawValidElement = true;
    if (inner === currentInner) return "match";
  }
  return sawValidElement ? "no-match" : "invalid";
}

export interface HistoryConditionalInput {
  responseBodyUtf8: string;
  revision: string | null;
  /** all=1 / reconciliation=1 不进入条件快路径（E01 304 资格）。 */
  forceAll: boolean;
  reconciling: boolean;
  /** 普通读取降级（legacy 全量兜底）= "full" → 不签发标签。 */
  mode: "directory" | "full" | "error";
  scope: {
    principalId: string | null;
    serverNodeId: string | null;
    studioId: string | null;
    sessionId: string | null;
    normalizedPath: string | null;
    branchIdentity: HistoryBranchIdentity | null;
  };
  beforeId: number | null;
  limit: number;
  ifNoneMatch: string | undefined | null;
  requestId: string | null;
}

export interface HistoryConditionalResult {
  status: 200 | 304;
  headers: Record<string, string>;
}

// ── E04：会话概览（复用 B/C 目录；契约冻结见 protocol-contract.md） ─────────

export interface HistoryOverview {
  schemaVersion: 1;
  available: true;
  /** 仅在既有兼容身份路径允许时为 null（path 直呼且无 manifest 映射）。 */
  sessionId: string | null;
  /** 对应目录的既有文件 revision，非整个响应版本。 */
  revision: string;
  counts: {
    /** B 的可展示原始记录计数（含被前端隐藏但推进 display 序号的记录）。 */
    displayRecords: number;
    /** 原 sourceMessages 坐标计数（当前分支；非物理 JSONL 行数）。 */
    sourceRecords: number;
    /** 至少含一条 displayable assistant 的 Run 数（仅用户输入尾段不计入）。 */
    runsWithAssistant: number;
    /** 当前分支经权威解析器识别的不同 taskId 数（不统计普通 toolCallId）。 */
    referencedTasks: number;
  };
  runSizeDistribution: {
    oneTo50: number;
    from51To200: number;
    over200: number;
  };
  taskDistribution: {
    subagent: number;
    workflow: number;
    media: number;
    other: number;
  };
  pagination: {
    legacyDefaultLimit: 50;
    recommendedLimit: number;
    maxLimit: 200;
    estimatedPagesAtRecommendedLimit: number;
  };
}

export interface HistoryOverviewUnavailable {
  schemaVersion: 1;
  available: false;
  reason: HistoryOverviewUnavailableReason;
}

export type HistoryOverviewUnavailableReason =
  | "revision_unknown"
  | "directory_unavailable"
  | "unsupported_history";

/**
 * 成功返回边界的条件求值：序列化已完成（调用方持有唯一字节串），这里只生成标签、
 * 协商头并判定 If-None-Match。非可签发响应仍返回 200 + 私有策略头（无 ETag/协议头）。
 */
export function evaluateHistoryConditionalGet(input: HistoryConditionalInput): HistoryConditionalResult {
  const headers: Record<string, string> = {
    "content-type": "application/json",
    "cache-control": "private, no-store",
  };
  const issuable = !input.forceAll && !input.reconciling && input.mode === "directory";
  if (!issuable) return { status: 200, headers };

  const tag = buildHistoryPageTag({
    protocolVersion: HISTORY_PROTOCOL_VERSION,
    scope: input.scope,
    endpoint: "/sessions/messages",
    before: input.beforeId,
    limit: input.limit,
    mode: "normal",
    language: "",
    revision: input.revision,
    responseBodyUtf8: input.responseBodyUtf8,
  });
  if (!tag) return { status: 200, headers };

  headers.etag = tag;
  headers["lingxi-history-protocol"] = String(HISTORY_PROTOCOL_VERSION);
  headers["lingxi-history-page-limit"] = String(HISTORY_PROTOCOL_PAGE_LIMIT);

  const verdict = evaluateIfNoneMatch(input.ifNoneMatch, tag);
  if (verdict === "match") {
    // E02.3：日志只含 requestId/结果/截短不可逆摘要（不含路径/查询/正文/凭证）。
    log.info(`history protocol conditional: request=${input.requestId ?? "-"} result=304 tag=${tag.slice(4, 20)}…`);
    // 304 无正文、无 Content-Type；私有策略与 ETag 必须在（E02.3）。
    const { "content-type": _drop, ...headers304 } = headers;
    return { status: 304, headers: headers304 };
  }
  return { status: 200, headers };
}
