/**
 * 历史读取 —— 投影事实扫描（阶段 B / B01 同源抽取）
 *
 * 把 /sessions/messages 路由主循环前的 7 组预扫描（origin zip、presentation、
 * agentReview、Run 边界、toolResult 定位、sourceIndex/displayIndex 索引、turn-input
 * consumption）合并为单个 visit pass，外加 finalize 的两个补充 pass（todos 反扫、
 * deferred-result 锚点前向判定）。这些 pass 相互独立（各自只读 sourceMessages），
 * 合并不改变任何 Map 内容；等价性由 A04 冻结参考差分
 * （scripts/diff-history-read-directory-phase-b.mjs）逐字节保证。
 *
 * 判定规则不在这里重写：origin/presentation/review 的注释结果消费现有
 * annotateOriginMessages，collab/modelCall/todos 直接调用
 * collectSessionCollabDecisions / collectModelCallReferencesBySourceIndex /
 * extractLatestTodoSnapshot 等现有纯函数。本模块只搬移循环体，并额外记录
 * recordFacts（每条记录进入主循环时刻的指针快照），作为后续目录构建（B03）
 * 的记录级事实来源——冷构建与无缓存全量路径因此共用同一套事实实现（I04）。
 */
import {
  contentHasThinkingBlock,
  annotateOriginMessages,
  collectSessionCollabDecisions,
  collectModelCallReferencesBySourceIndex,
} from "../../core/message-utils.ts";
import { isToolCallBlock } from "../../core/llm-utils.ts";
import { isAssistantCommentaryTextBlock } from "../../shared/text-signature.ts";
import {
  AGENT_REVIEW_RECORD_TYPE,
  MESSAGE_ORIGIN_RECORD_TYPE,
  MESSAGE_PRESENTATION_RECORD_TYPE,
} from "../../core/desktop-session-submit.ts";
import {
  TURN_INPUT_CONSUMPTION_EVENT_TYPE,
  isCustomTurnInputHistoryMessage,
  isHiddenTurnInputMessage,
  parseTurnInputConsumptionRecord,
} from "../../lib/turn-input-presentation.ts";
import { LOOP_TURN_MESSAGE_TYPE } from "../../lib/loop/loop-messages.ts";
import {
  DEFERRED_RESULT_MESSAGE_TYPE,
  DEFERRED_RESULT_RECORD_TYPE,
  parseDeferredResultNotification,
  parseDeferredResultRecord,
} from "../../lib/deferred-result-notification.ts";
import { extractLatestTodoSnapshot } from "../../lib/tools/todo-compat.ts";
import type { HistoryRecordFact } from "./types.ts";

function stripInlineThinkText(text) {
  return String(text || "").replace(/<think(?:ing)?>([\s\S]*?)<\/think(?:ing)?>\n*/g, "");
}

function hasInlineImageContent(content) {
  if (!Array.isArray(content)) return false;
  return content.some(block => block?.type === "image" && (block.data || block.source?.data));
}

function hasTextBlockContent(content, { stripThink = false } = {}) {
  if (typeof content === "string") {
    const text = stripThink ? stripInlineThinkText(content) : content;
    return text.length > 0;
  }
  if (!Array.isArray(content)) return false;
  return content.some(block => block?.type === "text" && block.text && !isAssistantCommentaryTextBlock(block));
}

function hasAssistantSemanticTextContent(content) {
  if (typeof content === "string") return stripInlineThinkText(content).length > 0;
  if (!Array.isArray(content)) return false;
  return content.some(block => block?.type === "text" && typeof block.text === "string" && block.text.length > 0);
}

function hasToolUseContent(content) {
  if (!Array.isArray(content)) return false;
  return content.some(block => (block?.type === "tool_use" || block?.type === "toolCall") && !!block.name);
}

export function isDisplayableHistoryMessage(message) {
  if (!message || typeof message !== "object") return false;
  if (message.role === "user") {
    return hasTextBlockContent(message.content) || hasInlineImageContent(message.content);
  }
  if (message.role === "assistant") {
    return message.stopReason === "error"
      || message.stopReason === "aborted"
      || hasAssistantSemanticTextContent(message.content)
      || contentHasThinkingBlock(message.content, { stripThink: true })
      || hasToolUseContent(message.content);
  }
  return false;
}

export function nextImmediateDisplayableAssistantIndex(sourceMessages, sourceIndex, displayIdxAtSource) {
  let displayIdx = displayIdxAtSource;
  for (let i = sourceIndex + 1; i < sourceMessages.length; i += 1) {
    const message = sourceMessages[i];
    if (!isDisplayableHistoryMessage(message)) continue;
    const currentIndex = displayIdx;
    displayIdx += 1;
    if (message.role === "user") return null;
    if (message.role === "assistant") return currentIndex;
  }
  return null;
}

export function isMediaGenerationDeferredResult(result) {
  return result?.type === "image-generation" || result?.type === "video-generation" || result?.type === "speech-generation";
}

export function parseHistoryDeferredResult(message) {
  if (message?.customType === DEFERRED_RESULT_RECORD_TYPE) {
    return parseDeferredResultRecord(message.data);
  }
  if (message?.customType === DEFERRED_RESULT_MESSAGE_TYPE) {
    return parseDeferredResultNotification(message.content);
  }
  return null;
}

export function historyDeferredDeliveryId(message, sourceIndex) {
  const details = message?.details && typeof message.details === "object" ? message.details : null;
  const fromDetails = typeof details?.deliveryId === "string" && details.deliveryId.trim()
    ? details.deliveryId.trim()
    : null;
  if (fromDetails) return fromDetails;
  return `history:${sourceIndex}`;
}

// 记录级事实类型（§3 记录级全集）定义在 ./types.ts——scanner 产出其 before 状态子集，
// 目录构建（directory.ts）补齐 toolCallIds/runOrdinal 等关联字段，同一类型（I04）。
export type { HistoryRecordFact } from "./types.ts";

export interface ProjectionContext {
  originBySourceIndex: Map<number, { origin: string; displayText?: string }>;
  presentationBySourceIndex: Map<number, any>;
  agentReviewBySourceIndex: Map<number, any>;
  /** 目录指针表（§5 #9）：user sourceIndex → 注释它的 custom 记录 sourceIndex（不驻留正文）。 */
  originRecordSourceIndexByUserSourceIndex: Map<number, number>;
  presentationRecordSourceIndexByUserSourceIndex: Map<number, number>;
  agentReviewRecordSourceIndexByUserSourceIndex: Map<number, number>;
  runBoundsBySourceIndex: Map<number, { start: number; end: number }>;
  /** assistant sourceIndex → 所属 Run 的序号（目录记录级 runOrdinal 字段用）。 */
  runOrdinalBySourceIndex: Map<number, number>;
  /** runOrdinal → {start,end}（C03 Run 小表：全量与增量同形状）。 */
  runBoundsByOrdinal: Map<number, { start: number; end: number }>;
  consumptionByAssistantEntryId: Map<string, number[]>;
  turnInputConsumptionDeliveryIds: Set<string>;
  turnInputConsumptionEntryIds: Set<string>;
  collabDecisionsBySuggestionId: Map<string, any>;
  modelCallReferenceBySourceIndex: Map<number, any>;
  toolResultSourceIndexByCallId: Map<string, number>;
  sourceIndexByEntryId: Map<string, number>;
  displayIndexByEntryId: Map<string, number>;
  turnInputByAssistantEntryId: Map<string, string>;
  mediaResultRecords: Array<{ sourceIndex: number; taskId: string; success: boolean }>;
  /** deferred-result 记录的锚点事实：key=sourceIndex，value=主循环 :1992-1998 的等价判定结果。 */
  deferredInterludeAnchors: Map<number, { sourceIndex: number; afterIndex: number | null; deliveryId: string | null }>;
  /** extractLatestTodoSnapshot 的返回值（快照事实本身）。 */
  todoSnapshot: ReturnType<typeof extractLatestTodoSnapshot>;
  /** todoSnapshot 选中记录的 sourceIndex（B03 指针；经单记录探针定位，不复制合法性规则）。 */
  todoSnapshotSourceIndex: number | null;
  recordFacts: HistoryRecordFact[];
  displayTotal: number;
  /** 扫描终态（C03 增量 seed）：指针/计数在全部记录处理完成后的值。 */
  finalTurnInputEntryId: string | null;
  finalTurnInputVisible: boolean;
  finalAssistantOrdinal: number;
  finalRunOrdinal: number;
}

export interface ProjectionFactScanner {
  visit(message: any, sourceIndex: number): void;
  finalize(): ProjectionContext;
}

/**
 * 流式事实收集器：冷构建（B03）逐条喂；无缓存全量路径经
 * buildProjectionContextFromMessages 对整个数组一次喂。同一实现。
 *
 * opts.sourceMessages / opts.annotatedOriginMessages 供 finalize 的前向/反向补充
 * pass 与 origin zip 使用——这些 pass 与 visit 一样只读数组，不含任何写路径。
 */
export function createProjectionFactScanner(opts: {
  sourceMessages?: any[];
  annotatedOriginMessages?: any[];
  /** C03 增量 seed：以上一版本尾部终态延续指针机（不重放旧记录）。缺省 = 零起点。 */
  seed?: {
    displayCounter: number;
    latestTurnInputEntryId: string | null;
    latestTurnInputVisible: boolean;
    assistantOrdinalInTurn: number;
    runOrdinal: number;
  };
} = {}): ProjectionFactScanner {
  const annotatedOriginMessages = opts.annotatedOriginMessages || null;

  // origin zip：跳过三类 custom 记录不清注释指针（annotateOriginMessages 的消费侧规则）。
  let annotatedIdx = 0;
  let pendingPresentation: any = null;
  let pendingReview: any = null;

  // Run 边界（displayCounter 与主循环 displayIdx 同源：只有 user/assistant 且
  // isDisplayableHistoryMessage 为真的消息推进序号——含被前端过滤的隐藏 user，
  // 它们同样开启新 Run）。
  let displayCounter = opts.seed?.displayCounter ?? 0;
  let runOrdinal = opts.seed?.runOrdinal ?? 0;
  const runOrdinalBySourceIndex = new Map<number, number>();
  const runBounds: Array<{ start?: number; end?: number }> = [];
  // C03：Run 小表与 consumption 反向索引（增量合并依据）
  const runBoundsByOrdinal = new Map<number, { start: number; end: number }>();
  const consumptionByAssistantEntryId = new Map<string, number[]>();

  // 指针状态（主循环入口时刻的快照进 recordFacts；易碎点 1/2：隐藏 user 占号、
  // 不可见 assistant 先加 ordinal 后判可见）。
  let latestTurnInputEntryId = opts.seed?.latestTurnInputEntryId ?? null;
  // 初始视为“可见”（零起点）：会话尚无输入时投影不应带 turnInputVisible:false。
  // 增量 seed 则延续上一版本尾部指针状态。
  let latestTurnInputVisible = opts.seed?.latestTurnInputVisible ?? true;
  let assistantOrdinalInTurn = opts.seed?.assistantOrdinalInTurn ?? 0;

  const originBySourceIndex = new Map();
  const presentationBySourceIndex = new Map();
  const agentReviewBySourceIndex = new Map();
  // 目录指针表：注释某条 user 的 custom 记录自身的 sourceIndex（§5 #9）。
  let lastOriginRecordSourceIndex: number | null = null;
  let lastPresentationRecordSourceIndex: number | null = null;
  let lastReviewRecordSourceIndex: number | null = null;
  const originRecordSourceIndexByUserSourceIndex = new Map<number, number>();
  const presentationRecordSourceIndexByUserSourceIndex = new Map<number, number>();
  const agentReviewRecordSourceIndexByUserSourceIndex = new Map<number, number>();
  const runBoundsBySourceIndex = new Map();
  const toolResultSourceIndexByCallId = new Map<string, number>();
  const sourceIndexByEntryId = new Map<string, number>();
  const displayIndexByEntryId = new Map<string, number>();
  const turnInputByAssistantEntryId = new Map<string, string>();
  const turnInputConsumptionDeliveryIds = new Set<string>();
  const turnInputConsumptionEntryIds = new Set<string>();
  const mediaResultRecords: Array<{ sourceIndex: number; taskId: string; success: boolean }> = [];
  const deferredResultRecordIndexes: number[] = [];
  const deferredDeliveryIdBySourceIndex = new Map<number, string | null>();
  const recordFacts: HistoryRecordFact[] = [];

  return {
    visit(message, sourceIndex) {
      const entryId = typeof message?.id === "string" && message.id.trim() ? message.id.trim() : null;
      const displayable = isDisplayableHistoryMessage(message);

      // —— 记录级 before 状态（visit 时随手记，即目录事实与全量上下文的同一产出机制）——
      const fact: HistoryRecordFact = {
        sourceIndex,
        entryId,
        role: message?.role ?? null,
        ...(message?.role === "custom" && message.customType ? { customType: message.customType } : {}),
        visible: displayable,
        displayIndex: null,
        displayIndexBefore: displayCounter,
        turnInputEntryIdBefore: latestTurnInputEntryId,
        turnInputVisibleBefore: latestTurnInputVisible,
        assistantOrdinalBefore: assistantOrdinalInTurn,
      };
      recordFacts.push(fact);

      // —— origin zip（消费 annotateOriginMessages 的注释结果，映射回原始下标，
      //     不破坏 sourceIndex 语义）——
      if (message?.role === "custom" && message.customType === MESSAGE_ORIGIN_RECORD_TYPE) {
        lastOriginRecordSourceIndex = sourceIndex;
      }
      if (message?.role === "custom" && message.customType === MESSAGE_PRESENTATION_RECORD_TYPE) {
        lastPresentationRecordSourceIndex = sourceIndex;
      }
      if (message?.role === "custom" && message.customType === AGENT_REVIEW_RECORD_TYPE) {
        lastReviewRecordSourceIndex = sourceIndex;
      }
      if (annotatedOriginMessages) {
        const isAnnotationRecord = message?.role === "custom" && (
          message.customType === MESSAGE_ORIGIN_RECORD_TYPE
          || message.customType === AGENT_REVIEW_RECORD_TYPE
          || message.customType === MESSAGE_PRESENTATION_RECORD_TYPE
        );
        if (!isAnnotationRecord) {
          const annotated = annotatedOriginMessages[annotatedIdx];
          annotatedIdx += 1;
          if (message?.role === "user" && annotated?.origin) {
            originBySourceIndex.set(sourceIndex, {
              origin: annotated.origin,
              ...(typeof annotated.displayText === "string" ? { displayText: annotated.displayText } : {}),
            });
            if (lastOriginRecordSourceIndex != null) {
              originRecordSourceIndexByUserSourceIndex.set(sourceIndex, lastOriginRecordSourceIndex);
            }
          }
        }
      }

      // —— presentation：注释其后第一条 user ——
      if (message?.role === "custom" && message.customType === MESSAGE_PRESENTATION_RECORD_TYPE) {
        pendingPresentation = message.data || null;
      } else if (message?.role === "user") {
        if (pendingPresentation) {
          presentationBySourceIndex.set(sourceIndex, pendingPresentation);
          if (lastPresentationRecordSourceIndex != null) {
            presentationRecordSourceIndexByUserSourceIndex.set(sourceIndex, lastPresentationRecordSourceIndex);
          }
        }
        pendingPresentation = null;
      }

      // —— agentReview：注释其后第一条 user，仅 status==="completed" 下发 ——
      if (message?.role === "custom" && message.customType === AGENT_REVIEW_RECORD_TYPE) {
        pendingReview = message.data || null;
      } else if (message?.role === "user") {
        if (pendingReview?.status === "completed") {
          agentReviewBySourceIndex.set(sourceIndex, pendingReview);
          if (lastReviewRecordSourceIndex != null) {
            agentReviewRecordSourceIndexByUserSourceIndex.set(sourceIndex, lastReviewRecordSourceIndex);
          }
        }
        pendingReview = null;
      }

      // —— Run 边界（分页 × Run 连续性，与主循环 latestTurnInput 指针逐字同源的判定）：
      // 为每个 displayable assistant 记录标注其所属 Run 的 display 序号区间
      // [turnStartIndex, turnEndIndex]。输入事件（user 消息 / custom turn input /
      // loop kickoff）开启新 Run；改任何一侧的边界判定必须同步另一侧。前端用它做两件事：
      //  1) 跨页归并：同 Run 的页片段按 (turnStartIndex, turnEndIndex) 识别并缝合，
      //     片段绝不各自派生 Run 终态（missing_final_answer 只能来自 Run 尾部）；
      //  2) 隐藏输入轮（loop 等 turnInputEntryId=null）也能正确按 Run 归并，
      //     不再退化为逐条记录各自投影。
      if (message?.role === "user") {
        runOrdinal += 1;
      } else if (message?.role === "custom" && (
        isCustomTurnInputHistoryMessage(message) || message.customType === LOOP_TURN_MESSAGE_TYPE
      )) {
        runOrdinal += 1;
      } else if (message?.role === "assistant" && displayable) {
        runOrdinalBySourceIndex.set(sourceIndex, runOrdinal);
        const bounds = runBounds[runOrdinal] || (runBounds[runOrdinal] = {});
        if (bounds.start === undefined) bounds.start = displayCounter;
        bounds.end = displayCounter;
        const runTableEntry = runBoundsByOrdinal.get(runOrdinal) || { start: displayCounter, end: displayCounter };
        runTableEntry.end = displayCounter;
        runBoundsByOrdinal.set(runOrdinal, runTableEntry);
      }

      // —— toolResult 定位 ——
      const toolCallId = typeof message?.toolCallId === "string" && message.toolCallId.trim()
        ? message.toolCallId.trim()
        : null;
      if (message?.role === "toolResult" && toolCallId) {
        toolResultSourceIndexByCallId.set(toolCallId, sourceIndex);
      }

      // —— entryId 索引 + display 序号 ——
      if (entryId) sourceIndexByEntryId.set(entryId, sourceIndex);
      if ((message?.role === "user" || message?.role === "assistant") && displayable) {
        if (entryId) displayIndexByEntryId.set(entryId, displayCounter);
        fact.displayIndex = displayCounter;
      }

      // —— turn-input consumption 解析 ——
      if (message?.role === "custom" && message.customType === TURN_INPUT_CONSUMPTION_EVENT_TYPE) {
        const parsed = parseTurnInputConsumptionRecord(message.data);
        const deliveryId = typeof parsed?.deliveryId === "string" && parsed.deliveryId.trim()
          ? parsed.deliveryId.trim()
          : null;
        const inputEntryId = typeof parsed?.input?.entryId === "string" && parsed.input.entryId.trim()
          ? parsed.input.entryId.trim()
          : null;
        const assistantEntryId = typeof parsed?.assistant?.entryId === "string" && parsed.assistant.entryId.trim()
          ? parsed.assistant.entryId.trim()
          : null;
        if (deliveryId) turnInputConsumptionDeliveryIds.add(deliveryId);
        if (inputEntryId) turnInputConsumptionEntryIds.add(inputEntryId);
        if (assistantEntryId && inputEntryId) turnInputByAssistantEntryId.set(assistantEntryId, inputEntryId);
        const consumptionLocals = consumptionByAssistantEntryId.get(assistantEntryId ?? inputEntryId ?? "") || [];
        consumptionLocals.push(sourceIndex);
        consumptionByAssistantEntryId.set(assistantEntryId ?? inputEntryId ?? "", consumptionLocals);
      }

      // —— custom 记录侧事实（media 结果、deferred-result 投递 id）——
      if (message?.role === "custom") {
        const parsedDeferred = parseHistoryDeferredResult(message);
        if (parsedDeferred?.taskId) {
          // E04：权威解析器识别的任务引用进目录事实（taskId + 自身元数据 type）。
          fact.deferredTaskRef = { taskId: parsedDeferred.taskId, type: parsedDeferred.type || "" };
        }
        if (parsedDeferred?.taskId && isMediaGenerationDeferredResult(parsedDeferred)) {
          mediaResultRecords.push({
            sourceIndex,
            taskId: parsedDeferred.taskId,
            success: parsedDeferred.status === "success",
          });
        }
        if (message.customType === DEFERRED_RESULT_MESSAGE_TYPE) {
          deferredResultRecordIndexes.push(sourceIndex);
          deferredDeliveryIdBySourceIndex.set(sourceIndex, historyDeferredDeliveryId(message, sourceIndex));
        }
      }

      // —— 指针推进（主循环同源规则；assistant 先加后判可见）——
      if (message?.role === "user") {
        assistantOrdinalInTurn = 0;
        latestTurnInputEntryId = entryId;
        latestTurnInputVisible = !isHiddenTurnInputMessage(message);
      } else {
        if (message?.role === "custom" && isCustomTurnInputHistoryMessage(message)) {
          assistantOrdinalInTurn = 0;
          latestTurnInputEntryId = entryId;
          latestTurnInputVisible = false;
        }
        // loop kickoff/wakeup 协议消息把输入指针置 null（重试入口裁剪规则见主循环）。
        if (message?.role === "custom" && message.customType === LOOP_TURN_MESSAGE_TYPE) {
          latestTurnInputEntryId = null;
          latestTurnInputVisible = false;
        }
        if (message?.role === "assistant") {
          assistantOrdinalInTurn += 1;
        }
      }
      if ((message?.role === "user" || message?.role === "assistant") && displayable) {
        displayCounter += 1;
      }
    },

    finalize(): ProjectionContext {
      const sourceMessages = opts.sourceMessages || [];

      // 会话级事实：直接调用现有纯函数（不复制规则）。
      const collabDecisionsBySuggestionId = collectSessionCollabDecisions(sourceMessages);
      const modelCallReferenceBySourceIndex = collectModelCallReferencesBySourceIndex(sourceMessages);

      // todos 反向扫描：调用现有 extractLatestTodoSnapshot（不复制「最后者胜/坏快照跳过」
      // 规则），只记快照事实；指向具体记录的指针由 B03 目录构建层补充。
      const todoSnapshot = sourceMessages.length ? extractLatestTodoSnapshot(sourceMessages) : null;

      for (const [sourceIndex, ordinal] of runOrdinalBySourceIndex) {
        runBoundsBySourceIndex.set(sourceIndex, runBounds[ordinal]);
      }
      // Run 小表与 consumption 反向索引已在 visit 中随扫描填充

      // 记录级关联/Run 字段补齐（冷目录与无缓存全量共用同一事实形状，I04）：
      // assistant 的 toolUse id 与 extractTextContent 的 toolUses 同一判定
      // （isToolCallBlock），但不做 text/thinking 提取与参数摘要；热页据此在
      // resolveHistoryPage 阶段补定位跨页 toolResult 结局记录（§5 #5）。
      for (let sourceIndex = 0; sourceIndex < recordFacts.length; sourceIndex += 1) {
        const fact = recordFacts[sourceIndex];
        const message = sourceMessages[sourceIndex];
        const bounds = runBoundsBySourceIndex.get(sourceIndex);
        const runOrdinal = runOrdinalBySourceIndex.get(sourceIndex);
        if (message?.role === "assistant" && Array.isArray(message.content)) {
          const toolUseIds = message.content
            .filter((block: any) => isToolCallBlock(block) && typeof block?.id === "string" && block.id)
            .map((block: any) => block.id);
          if (toolUseIds.length) fact.toolCallIds = toolUseIds;
        }
        if (message?.role === "toolResult" && typeof message.toolCallId === "string" && message.toolCallId.trim()) {
          fact.toolCallIds = [message.toolCallId.trim()];
        }
        if (message?.role === "toolResult" && typeof message.toolName === "string" && message.toolName) {
          fact.toolName = message.toolName;
        }
        if (message?.role === "toolResult" && message.isError === true) fact.isError = true;
        if (runOrdinal != null) fact.runOrdinal = runOrdinal;
        if (bounds) {
          fact.turnStartIndex = bounds.start;
          fact.turnEndIndex = bounds.end;
        }
        if (typeof message?.timestamp === "string" && message.timestamp) fact.timestamp = message.timestamp;
      }

      // deferred-result 锚点前向判定（与旧主循环 nextImmediateDisplayableAssistantIndex
      // 调用逐字同源；displayIdxAtSource = 该记录进入主循环时的 displayIdx）。
      const deferredInterludeAnchors = new Map();
      for (const sourceIndex of deferredResultRecordIndexes) {
        const displayIdxAtSource = recordFacts[sourceIndex].displayIndexBefore;
        const nextAssistantIndex = nextImmediateDisplayableAssistantIndex(sourceMessages, sourceIndex, displayIdxAtSource);
        deferredInterludeAnchors.set(sourceIndex, {
          sourceIndex,
          afterIndex: nextAssistantIndex == null ? null : nextAssistantIndex - 1,
          deliveryId: deferredDeliveryIdBySourceIndex.get(sourceIndex) ?? null,
        });
      }

      // todo 快照记录指针：合法性判定留在 extractLatestTodoSnapshot（不复制规则）。
      // 定位方式 = 从尾向前做单记录探针：extractLatestTodoSnapshot 选中「从尾起第一条
      // 合法快照」，而任何后于它的记录都不可能产出非 null 快照（否则被选中的会是它），
      // 所以首个单记录快照与整体快照 deep-equal 的记录即所选记录。夹具无 todos 时
      // todoSnapshot 为 null，探针整体跳过。
      let todoSnapshotSourceIndex: number | null = null;
      if (todoSnapshot) {
        const expected = JSON.stringify(todoSnapshot);
        for (let i = sourceMessages.length - 1; i >= 0; i -= 1) {
          const single = extractLatestTodoSnapshot([sourceMessages[i]]);
          if (single && JSON.stringify(single) === expected) {
            todoSnapshotSourceIndex = i;
            break;
          }
        }
      }

      return {
        originBySourceIndex,
        presentationBySourceIndex,
        agentReviewBySourceIndex,
        originRecordSourceIndexByUserSourceIndex,
        presentationRecordSourceIndexByUserSourceIndex,
        agentReviewRecordSourceIndexByUserSourceIndex,
        runBoundsBySourceIndex,
        runOrdinalBySourceIndex,
        collabDecisionsBySuggestionId,
        modelCallReferenceBySourceIndex,
        toolResultSourceIndexByCallId,
        sourceIndexByEntryId,
        displayIndexByEntryId,
        turnInputByAssistantEntryId,
        turnInputConsumptionDeliveryIds,
        turnInputConsumptionEntryIds,
        mediaResultRecords,
        deferredInterludeAnchors,
        todoSnapshot,
        todoSnapshotSourceIndex,
        recordFacts,
        displayTotal: displayCounter,
        finalTurnInputEntryId: latestTurnInputEntryId,
        finalTurnInputVisible: latestTurnInputVisible,
        finalAssistantOrdinal: assistantOrdinalInTurn,
        finalRunOrdinal: runOrdinal,
        runBoundsByOrdinal,
        consumptionByAssistantEntryId,
      };
    },
  };
}

/** 无缓存全量路径的入口：对整个 sourceMessages 数组一次 visit + finalize。 */
export function buildProjectionContextFromMessages(sourceMessages: any[]): ProjectionContext {
  // annotateOriginMessages 会把 origin custom 条目从数组里摘掉、把 origin/displayText
  // 并进其后第一条 user 消息。主展示循环大量以 sourceIndex 回查 sourceMessages
  // （nextImmediateDisplayableAssistantIndex、recordDeferredInterlude 等），不能直接
  // 换成过滤后的短数组。这里沿用 zip 只取注释结果、映射回原始下标的方式，循环本身
  // 仍遍历原始 sourceMessages，不破坏既有 sourceIndex 语义。
  const annotatedOriginMessages = annotateOriginMessages(sourceMessages);
  const scanner = createProjectionFactScanner({ sourceMessages, annotatedOriginMessages });
  for (let sourceIndex = 0; sourceIndex < sourceMessages.length; sourceIndex += 1) {
    scanner.visit(sourceMessages[sourceIndex], sourceIndex);
  }
  return scanner.finalize();
}
