/**
 * 历史读取 —— 页面投影（阶段 B / B01 同源抽取）
 *
 * /sessions/messages 主循环（user / assistant / toolResult / custom 四分支）整体
 * 迁出：四分支判定、afterIndex = displayIdx - 1、origin → agentReview → presentation
 * 的 spread 次序、assistantOrdinal 先加后判可见、consumption 覆盖输入指针、loop 置
 * null、`assistant:${ordinal}` 段 id、Run 边界挂接，全部逐分支原样搬移，不改任何判定。
 * 全量路径与热页路径共用本函数（I04）：全量喂 iterate=全部记录 + seed=零起点；
 * 热页喂窗口∪页内锚点记录 + 页首 before 状态种子。
 */
import { extractTextContent, contentHasThinkingBlock, filterUnreferencedInlineImages, overlaySessionCollabDecision } from "../../core/message-utils.ts";
import { extractPersistedAssistantSemanticSegments } from "../../shared/assistant-semantic-segments.ts";
import { projectToolPresentationDetails, safeToolInput, toolResultText } from "../../shared/tool-presentation.ts";
import { extractBlocks } from "../block-extractors.ts";
import { createHistoryDeferredContentFor, shouldDeferHistoryContent } from "../history-deferred-content.ts";
import { buildDeferredResultInterludeBlock } from "../deferred-result-interlude.ts";
import {
  TURN_INPUT_CONSUMPTION_EVENT_TYPE,
  TURN_INPUT_PRESENTATION_EVENT_TYPE,
  isCustomTurnInputHistoryMessage,
  isHiddenTurnInputMessage,
  parseTurnInputConsumptionRecord,
  parseTurnInputPresentationRecord,
} from "../../lib/turn-input-presentation.ts";
import {
  LOOP_TURN_MESSAGE_TYPE,
  LOOP_NOTICE_MESSAGE_TYPE,
  buildLoopInterludeBlock,
} from "../../lib/loop/loop-messages.ts";
import { DEFERRED_RESULT_MESSAGE_TYPE } from "../../lib/deferred-result-notification.ts";
import { stripSessionReminderBlocks } from "../../core/session-reminders.ts";
import { sanitizeBridgeVisibleText } from "../../shared/bridge-visible-text.ts";
import {
  historyDeferredDeliveryId,
  isDisplayableHistoryMessage,
  isMediaGenerationDeferredResult,
  parseHistoryDeferredResult,
  type ProjectionContext,
} from "./projection-context.ts";

/** 稀疏记录视图（I02：不重新编号）。全量路径 = 恒等视图；热页 = 按 sourceIndex 的 Map 视图。 */
export interface HistoryRecordView {
  length: number;
  get(sourceIndex: number): any;
}

/** 全量路径的恒等视图：get(i) === sourceMessages[i]。 */
export function identityRecordView(sourceMessages: any[]): HistoryRecordView {
  return {
    length: sourceMessages.length,
    get: (sourceIndex: number) => sourceMessages[sourceIndex],
  };
}

export function soleRawToolResultText(message) {
  return toolResultText(message);
}

export function deferHeavyHistoryBlock(records: HistoryRecordView, sourceIndex: number, ordinal: number, block) {
  if (block.type === "screenshot" && shouldDeferHistoryContent(block.base64)) {
    const { base64, ...rest } = block;
    return {
      ...rest,
      deferred: createHistoryDeferredContentFor(
        records.get(sourceIndex),
        sourceIndex,
        "screenshot",
        ordinal,
        base64,
        { preview: false },
      ),
    };
  }
  if (block.type === "artifact" && shouldDeferHistoryContent(block.content)) {
    const deferred = createHistoryDeferredContentFor(
      records.get(sourceIndex),
      sourceIndex,
      "artifact",
      ordinal,
      block.content,
    );
    return { ...block, content: deferred.preview || "", deferred };
  }
  return block;
}

export function isBridgeSessionPath(sessionPath) {
  if (typeof sessionPath !== "string" || !sessionPath) return false;
  return sessionPath.split(/[\\/]+/).includes("bridge");
}

export function createSanitizeVisibleContent(sessionPath) {
  return (value) => {
    const withoutReminder = stripSessionReminderBlocks(value);
    return isBridgeSessionPath(sessionPath)
      ? sanitizeBridgeVisibleText(withoutReminder)
      : withoutReminder;
  };
}

export function taskFromSubagentRun(run) {
  if (!run) return null;
  return {
    status: run.status,
    result: run.summary || null,
    reason: run.reason || run.summary || null,
    meta: {
      sessionId: run.childSessionId || null,
      sessionPath: run.childSessionPath || null,
      requestedAgentId: run.requestedAgentId || null,
      requestedAgentNameSnapshot: run.requestedAgentNameSnapshot || null,
      executorAgentId: run.executorAgentId || null,
      executorAgentNameSnapshot: run.executorAgentNameSnapshot || null,
      executorMetaVersion: run.executorMetaVersion || null,
    },
  };
}

export function mergeSubagentTaskMetadata(primary, fallback) {
  if (!primary) return fallback || null;
  if (!fallback) return primary;
  const primaryMeta = {};
  for (const [key, value] of Object.entries(primary.meta || {})) {
    if (value != null) primaryMeta[key] = value;
  }
  return {
    status: primary.status || fallback.status,
    result: primary.result ?? fallback.result,
    reason: primary.reason ?? fallback.reason,
    meta: {
      ...(fallback.meta || {}),
      ...primaryMeta,
    },
  };
}

export interface PageProjectorInput {
  /**
   * 记录视图：get(sourceIndex) → 原始投影消息（缺失返回 null/undefined）。
   * 全量路径 = identityRecordView(sourceMessages)；热页 = 窗口∪闭包读取结果的
   * Map 视图（稀疏、不重新编号，I02）。deferred 凭证经 createHistoryDeferredContentFor
   * 直取单条记录，locator 编码与数组入口完全一致。
   */
  records: HistoryRecordView;
  context: ProjectionContext;
  bounds: { total: number; startIdx: number; endIdx: number; hasMore: boolean };
  /** 页首种子：全量路径为 { 0, null, true, 0 }（与旧主循环初始值逐字一致）；热页为目录记录的 before 状态。 */
  seed: {
    displayIdx: number;
    latestTurnInputEntryId: string | null;
    latestTurnInputVisible: boolean;
    assistantOrdinalInTurn: number;
  };
  /** 本页要遍历的记录（全量=全部记录；热页=窗口∪页内锚点命中记录，按 sourceIndex 升序）。 */
  iterate: Array<{ sourceIndex: number; message: any }>;
  sanitizeVisibleContent: (value: string) => string;
  toolOutcomesByCallId: Map<string, any>;
  engine: any;
  receiverName: string | null;
}

export interface ProjectedPage {
  messages: any[];
  blocks: any[];
  mediaGenerationResults: Map<string, any>;
  standaloneMediaGenerationResults: any[];
  /** 主循环后段（deferred store 回灌）复用同一闭包实例——共享 blocks / 去重集合。 */
  recordMediaGenerationResult: (parsed: any, afterIndex: number, sourceIndex?: number | null) => void;
  recordDeferredInterlude: (parsed: any, afterIndex: number | null, deliveryId?: string | null, sourceIndex?: number | null) => void;
}

export function projectHistoryPage(input: PageProjectorInput): ProjectedPage {
  const {
    records,
    context,
    bounds: pageBounds,
    seed,
    iterate,
    sanitizeVisibleContent,
    toolOutcomesByCallId,
    engine,
    receiverName,
  } = input;
  const deferredStore = engine.deferredResults;
  const messages: any[] = [];
  const blocks: any[] = [];
  const mediaGenerationResults = new Map();
  const standaloneMediaGenerationResults: any[] = [];
  const deferredInterludeDeliveryIds = new Set<string>();
  const recordMediaGenerationResult = (parsed, afterIndex, sourceIndex = null) => {
    if (!parsed?.taskId || !isMediaGenerationDeferredResult(parsed)) return;
    mediaGenerationResults.set(parsed.taskId, parsed);
    if (parsed.status === "success") {
      standaloneMediaGenerationResults.push({
        ...parsed,
        afterIndex,
        ...(Number.isInteger(sourceIndex) ? { sourceIndex } : {}),
      });
    }
  };
  const recordTurnInputConsumptionInterlude = (message, afterIndex, sourceIndex = null) => {
    const parsed = parseTurnInputConsumptionRecord(message?.data);
    const block = parsed?.block;
    if (!block || block.type !== "interlude") return;
    const assistantEntryId = typeof parsed?.assistant?.entryId === "string" && parsed.assistant.entryId.trim()
      ? parsed.assistant.entryId.trim()
      : null;
    const inputEntryId = typeof parsed?.input?.entryId === "string" && parsed.input.entryId.trim()
      ? parsed.input.entryId.trim()
      : null;
    const assistantDisplayIndex = assistantEntryId
      ? context.displayIndexByEntryId.get(assistantEntryId)
      : undefined;
    const anchoredAfterIndex = Number.isInteger(assistantDisplayIndex)
      ? Math.max(0, assistantDisplayIndex - 1)
      : afterIndex;
    if (!Number.isInteger(anchoredAfterIndex) || anchoredAfterIndex < 0) return;
    const inputSourceIndex = inputEntryId ? context.sourceIndexByEntryId.get(inputEntryId) : undefined;
    const anchoredSourceIndex = Number.isInteger(inputSourceIndex) ? inputSourceIndex : sourceIndex;
    const normalizedDeliveryId = typeof parsed.deliveryId === "string" && parsed.deliveryId.trim()
      ? parsed.deliveryId.trim()
      : null;
    if (normalizedDeliveryId && deferredInterludeDeliveryIds.has(normalizedDeliveryId)) return;
    blocks.push({
      ...block,
      ...(normalizedDeliveryId ? { deliveryId: normalizedDeliveryId } : {}),
      afterIndex: anchoredAfterIndex,
      ...(Number.isInteger(anchoredSourceIndex) ? { sourceIndex: anchoredSourceIndex } : {}),
    });
    if (normalizedDeliveryId) deferredInterludeDeliveryIds.add(normalizedDeliveryId);
  };
  const recordTurnInputPresentationInterlude = (message, afterIndex, sourceIndex = null) => {
    if (!Number.isInteger(afterIndex) || afterIndex < 0) return;
    const parsed = parseTurnInputPresentationRecord(message?.data);
    const block = parsed?.block;
    if (!block || block.type !== "interlude") return;
    const normalizedDeliveryId = typeof parsed.deliveryId === "string" && parsed.deliveryId.trim()
      ? parsed.deliveryId.trim()
      : null;
    if (normalizedDeliveryId && deferredInterludeDeliveryIds.has(normalizedDeliveryId)) return;
    blocks.push({
      ...block,
      ...(normalizedDeliveryId ? { deliveryId: normalizedDeliveryId } : {}),
      afterIndex,
      ...(Number.isInteger(sourceIndex) ? { sourceIndex } : {}),
    });
    if (normalizedDeliveryId) deferredInterludeDeliveryIds.add(normalizedDeliveryId);
  };
  const recordDeferredInterlude = (parsed, afterIndex, deliveryId = null, sourceIndex = null) => {
    if (!parsed?.taskId || !Number.isInteger(afterIndex) || afterIndex < 0) return;
    const normalizedDeliveryId = typeof deliveryId === "string" && deliveryId.trim() ? deliveryId.trim() : null;
    const sourceMessage = Number.isInteger(sourceIndex) ? records.get(sourceIndex) : null;
    const sourceEntryId = typeof sourceMessage?.id === "string" && sourceMessage.id.trim()
      ? sourceMessage.id.trim()
      : null;
    if (normalizedDeliveryId && context.turnInputConsumptionDeliveryIds.has(normalizedDeliveryId)) return;
    if (sourceEntryId && context.turnInputConsumptionEntryIds.has(sourceEntryId)) return;
    if (normalizedDeliveryId && deferredInterludeDeliveryIds.has(normalizedDeliveryId)) return;
    const task = deferredStore?.query?.(parsed.taskId) || null;
    const run = engine.subagentRuns?.query?.(parsed.taskId) || null;
    const runTask = taskFromSubagentRun(run);
    const metadataTask = mergeSubagentTaskMetadata(runTask, task);
    const metadataMeta = metadataTask?.meta || {};
    const meta = {
      ...metadataMeta,
      type: parsed.type || metadataMeta.type || task?.meta?.type || "background-task",
    };
    const event = {
      taskId: parsed.taskId,
      deliveryId: normalizedDeliveryId,
      status: parsed.status === "failed" || parsed.status === "aborted" ? parsed.status : "success",
      result: Object.prototype.hasOwnProperty.call(parsed, "result") ? parsed.result : metadataTask?.result,
      reason: parsed.reason || metadataTask?.reason || null,
      meta,
    };
    const block = buildDeferredResultInterludeBlock(event, { receiverName });
    if (!block) return;
    blocks.push({
      ...block,
      afterIndex,
      ...(Number.isInteger(sourceIndex) ? { sourceIndex } : {}),
    });
    if (normalizedDeliveryId) deferredInterludeDeliveryIds.add(normalizedDeliveryId);
  };
  const recordLoopInterlude = (message, afterIndex, sourceIndex = null) => {
    // 循环任务的 kickoff/wakeup/notice 协议消息本身 display:false（含系统协议文本，
    // 不宜直接展示）。这里把它提炼成一条用户可见的 interlude 气泡，让用户能看到自己
    // 当初发起的任务，否则会在聊天界面以为"输入凭空消失"。block 构造与实时路径共用
    // buildLoopInterludeBlock，保证文案一致；实时路径的 dedup id 说明见该函数注释。
    if (!Number.isInteger(afterIndex) || afterIndex < 0) return;
    const block = buildLoopInterludeBlock(message);
    if (!block) return;
    blocks.push({
      ...block,
      afterIndex,
      ...(Number.isInteger(sourceIndex) ? { sourceIndex } : {}),
    });
  };
  let displayIdx = seed.displayIdx;
  let latestTurnInputEntryId: string | null = seed.latestTurnInputEntryId;
  // 初始视为“可见”：会话尚无输入时投影不应带 turnInputVisible:false（如角色卡
  // 开场白）。只有真实出现过隐藏输入（隐藏 user 消息 / 隐藏 custom 输入 / loop
  // 协议消息置 null）时才为 false，从而对 entryId 为 null 的隐藏轮也显式下发。
  let latestTurnInputVisible = seed.latestTurnInputVisible;
  let assistantOrdinalInTurn = seed.assistantOrdinalInTurn;

  for (const { sourceIndex, message: m } of iterate) {
    if (m.role === "user") {
      assistantOrdinalInTurn = 0;
      latestTurnInputEntryId = typeof m.id === "string" && m.id.trim() ? m.id.trim() : null;
      latestTurnInputVisible = !isHiddenTurnInputMessage(m);
      if (!isDisplayableHistoryMessage(m)) continue;
      const currentIndex = displayIdx;
      displayIdx += 1;
      if (currentIndex >= pageBounds.startIdx && currentIndex < pageBounds.endIdx) {
        const { text, images } = extractTextContent(m.content);
        const visibleImages = filterUnreferencedInlineImages(text, images).map((image, ordinal) => {
          if (!shouldDeferHistoryContent(image?.data)) return image;
          const { data, ...rest } = image;
          return {
            ...rest,
            deferred: createHistoryDeferredContentFor(
              records.get(sourceIndex),
              sourceIndex,
              "inline_image",
              ordinal,
              data,
              { preview: false },
            ),
          };
        });
        const content = sanitizeVisibleContent(text);
        const originInfo = context.originBySourceIndex.get(sourceIndex);
        const agentReview = context.agentReviewBySourceIndex.get(sourceIndex);
        const presentation = context.presentationBySourceIndex.get(sourceIndex);
        messages.push({
          id: String(currentIndex),
          sourceIndex,
          ...(m.id ? { entryId: m.id } : {}),
          role: "user",
          content,
          ...(m.clientMessageId ? { clientMessageId: m.clientMessageId, sourceEntryId: m.sourceEntryId, snapshotVersion: m.snapshotVersion } : {}),
          ...(m.acceptanceDiagnostic ? { acceptanceDiagnostic: m.acceptanceDiagnostic } : {}),
          images: visibleImages.length ? visibleImages : undefined,
          ...(m.timestamp ? { timestamp: m.timestamp } : {}),
          ...(originInfo?.origin ? { origin: originInfo.origin } : {}),
          ...(typeof originInfo?.displayText === "string" ? { displayText: originInfo.displayText } : {}),
          ...(agentReview ? { agentReview } : {}),
          ...(typeof agentReview?.displayText === "string" ? { displayText: agentReview.displayText } : {}),
          ...(typeof presentation?.displayText === "string" ? { displayText: presentation.displayText } : {}),
          ...(Array.isArray(presentation?.skills) ? { skills: presentation.skills } : {}),
          ...(Array.isArray(presentation?.sessionRefs) ? { sessionRefs: presentation.sessionRefs } : {}),
          ...(Array.isArray(presentation?.agentMentions) ? { agentMentions: presentation.agentMentions } : {}),
          ...(presentation?.knowledgeRefs ? { knowledgeRefs: presentation.knowledgeRefs } : {}),
          ...(presentation?.knowledgeRetrieval ? { knowledgeRetrieval: presentation.knowledgeRetrieval } : {}),
          ...(presentation?.agentReviewRequest ? { agentReviewRequest: presentation.agentReviewRequest } : {}),
        });
      }
    } else if (m.role === "assistant") {
      assistantOrdinalInTurn += 1;
      if (!isDisplayableHistoryMessage(m)) continue;
      const assistantEntryId = typeof m.id === "string" && m.id.trim() ? m.id.trim() : null;
      const consumedTurnInputEntryId = assistantEntryId
        ? context.turnInputByAssistantEntryId.get(assistantEntryId) || null
        : null;
      const turnInputEntryId = consumedTurnInputEntryId || latestTurnInputEntryId;
      const turnInputVisible = consumedTurnInputEntryId ? false : latestTurnInputVisible;
      const currentIndex = displayIdx;
      displayIdx += 1;
      if (currentIndex >= pageBounds.startIdx && currentIndex < pageBounds.endIdx) {
        const { text, thinking, toolUses } = extractTextContent(m.content, { stripThink: true });
        const extractedAssistantSegments = extractPersistedAssistantSemanticSegments(
          m.content,
          assistantOrdinalInTurn,
        );
        const assistantSegments = extractedAssistantSegments.map((segment, ordinal) => {
          if (segment.kind !== "reasoning" || !shouldDeferHistoryContent(segment.source)) return segment;
          const deferred = createHistoryDeferredContentFor(
            records.get(sourceIndex),
            sourceIndex,
            "assistant_segment",
            ordinal,
            segment.source,
          );
          return { ...segment, source: deferred.preview || "", deferred };
        });
        const turnStatus = m.stopReason === "error"
          ? "failed"
          : m.stopReason === "aborted"
            ? "aborted"
            : "completed";
        const content = sanitizeVisibleContent(text);
        const projectedToolUses = toolUses.map((toolUse) => {
          const outcome = toolUse.id ? toolOutcomesByCallId.get(toolUse.id) : null;
          const outcomeSourceIndex = toolUse.id
            ? context.toolResultSourceIndexByCallId.get(toolUse.id)
            : undefined;
          const rawCalls = Array.isArray(m.content) ? m.content : [];
          const callOrdinal = rawCalls.findIndex((block) => (
            (block?.type === "toolCall" || block?.type === "tool_use")
            && (toolUse.id ? block.id === toolUse.id : rawCalls.indexOf(block) === toolUse.processOrder)
          ));
          const rawCall = rawCalls[callOrdinal];
          const rawArgs = rawCall?.input ?? rawCall?.arguments ?? rawCall?.args ?? toolUse.args;
          const rawResult = Number.isInteger(outcomeSourceIndex) ? records.get(outcomeSourceIndex) : null;
          const fullPresentation = rawResult ? projectToolPresentationDetails({
            ...rawResult,
            isError: outcome?.status === "failed" || rawResult.isError === true,
          }, { toolName: toolUse.name, args: rawArgs }, Infinity) : safeToolInput(toolUse.name, rawArgs, Infinity);
          const details = { ...outcome?.details, ...fullPresentation };
          // 历史先从实际保存内容重新投影，取消仅由实时传输上限造成的标记。
          if (fullPresentation?.input !== undefined) delete details.inputTruncated;
          if (fullPresentation && "output" in fullPresentation) delete details.outputTruncated;
          if (details.execCommand?.tty === true) delete details.output;
          if (typeof details.input === "string" && shouldDeferHistoryContent(details.input) && callOrdinal >= 0) {
            const deferred = createHistoryDeferredContentFor(m, sourceIndex, "tool_input", callOrdinal, details.input);
            details.input = safeToolInput(toolUse.name, rawArgs, 240)?.input || "{}";
            details.inputDeferred = deferred;
          }
          if (rawResult && Number.isInteger(outcomeSourceIndex)) {
            if (typeof details.output === "string" && shouldDeferHistoryContent(details.output)) {
              const deferred = createHistoryDeferredContentFor(rawResult, outcomeSourceIndex, "tool_output", 0, details.output);
              details.output = deferred.preview || "";
              details.outputDeferred = deferred;
              // 搜索结构（path / line / context / matchCount / fileCount）是本次执行已有的
              // 事实，首包只为体积省掉 files；省掉的是体积，不是真相，所以另给一条可加载
              // 引用，展开时按原结构取回，而不是让前端从 output 文本里重新猜路径和行号。
              if (details.search && Array.isArray(details.search.files) && details.search.files.length) {
                const searchDeferred = createHistoryDeferredContentFor(
                  rawResult,
                  outcomeSourceIndex,
                  "tool_search",
                  0,
                  JSON.stringify(details.search),
                  // 结构预览会把刚刚省下的体积原样塞回首包；统计仍由 matchCount /
                  // fileCount 字段承担，引用只负责按需取回完整结构。
                  { preview: false },
                );
                details.search = { ...details.search, files: undefined, searchDeferred };
              }
            }
            const rawResultContent = soleRawToolResultText(rawResult);
            if (details.skillInvocation && shouldDeferHistoryContent(rawResultContent)) {
              const deferred = createHistoryDeferredContentFor(rawResult, outcomeSourceIndex, "skill_content", 0, rawResultContent);
              details.skillInvocation = { ...details.skillInvocation, content: deferred.preview || "", truncated: false, deferred };
            }
            if (details.fileChange) {
              const change = { ...details.fileChange };
              if (shouldDeferHistoryContent(change.patch)) {
                const deferred = createHistoryDeferredContentFor(rawResult, outcomeSourceIndex, "tool_patch", 0, change.patch);
                change.patch = deferred.preview || "";
                change.patchDeferred = deferred;
              }
              if (shouldDeferHistoryContent(change.content)) {
                const hasResultContent = typeof rawResult.details?.fileChange?.content === "string";
                if (hasResultContent || callOrdinal >= 0) {
                  const deferred = createHistoryDeferredContentFor(
                    hasResultContent ? rawResult : m,
                    hasResultContent ? outcomeSourceIndex : sourceIndex,
                    "tool_file_content",
                    hasResultContent ? 0 : callOrdinal,
                    change.content,
                  );
                  change.content = deferred.preview || "";
                  change.contentDeferred = deferred;
                }
              }
              details.fileChange = change;
            }
          }
          const projectedOutcome = {
            ...(outcome || { status: "unknown", success: false }),
            ...(Object.keys(details).length ? { details } : {}),
          };
          // 工具计时（dsh 轨迹视图同款「Session timestamps」口径）：
          // startedAt = 携带 tool_use 的 assistant 条目时间（工具执行前落盘），
          // endedAt = 对应 toolResult 条目时间（执行后落盘）。
          const toolResultTimestamp = outcome !== null
            && Number.isInteger(outcomeSourceIndex)
            ? records.get(outcomeSourceIndex)?.timestamp
            : undefined;
          return {
            ...toolUse,
            ...(m.timestamp ? { startedAt: m.timestamp } : {}),
            ...(toolResultTimestamp ? { endedAt: toolResultTimestamp } : {}),
            ...(projectedOutcome || { status: "unknown", success: false }),
          };
        });
        const deferredThinking = assistantSegments.find((segment) => segment.kind === "reasoning")?.source;
        const runBounds = context.runBoundsBySourceIndex.get(sourceIndex);
        messages.push({
          id: String(currentIndex),
          sourceIndex,
          ...(m.id ? { entryId: m.id } : {}),
          role: "assistant",
          content,
          ...(runBounds ? { turnStartIndex: runBounds.start, turnEndIndex: runBounds.end } : {}),
          ...(context.modelCallReferenceBySourceIndex.has(sourceIndex)
            ? { modelCallRef: context.modelCallReferenceBySourceIndex.get(sourceIndex) }
            : {}),
          assistantSegments,
          ...(turnStatus !== "completed" ? { turnStatus } : {}),
          ...(turnInputEntryId
            ? { turnInputEntryId, turnInputVisible }
            // 隐藏输入轮次（如 loop 轮）entryId 被刻意置 null，但仍要显式下发
            // turnInputVisible:false，否则前端技能卡「参数」会回退猜成前一条可见用户消息。
            : (turnInputVisible === false ? { turnInputVisible: false } : {})),
          ...(contentHasThinkingBlock(m.content, { stripThink: true })
            ? { thinking: deferredThinking ?? thinking }
            : {}),
          toolCalls: projectedToolUses.length ? projectedToolUses : undefined,
          ...(m.timestamp ? { timestamp: m.timestamp } : {}),
        });
      }
    } else if (m.role === "toolResult") {
      const afterIndex = displayIdx - 1;
      if (afterIndex >= pageBounds.startIdx && afterIndex < pageBounds.endIdx) {
        const extracted = extractBlocks(m.toolName, m.details, m);
        for (let ordinal = 0; ordinal < extracted.length; ordinal += 1) {
          const b = extracted[ordinal];
          const overlaid = overlaySessionCollabDecision(b, context.collabDecisionsBySuggestionId);
          blocks.push({
            ...deferHeavyHistoryBlock(records, sourceIndex, ordinal, overlaid),
            afterIndex,
            sourceIndex,
          });
        }
      }
    } else if (m.role === "custom") {
      if (isCustomTurnInputHistoryMessage(m)) {
        assistantOrdinalInTurn = 0;
        latestTurnInputEntryId = typeof m.id === "string" && m.id.trim() ? m.id.trim() : null;
        latestTurnInputVisible = false;
      }
      const afterIndex = displayIdx - 1;
      if (m.display !== false && afterIndex >= pageBounds.startIdx && afterIndex < pageBounds.endIdx) {
        const extracted = extractBlocks(m.customType, m.details, m);
        for (let ordinal = 0; ordinal < extracted.length; ordinal += 1) {
          const b = extracted[ordinal];
          blocks.push({
            ...deferHeavyHistoryBlock(records, sourceIndex, ordinal, b),
            afterIndex,
            sourceIndex,
          });
        }
      }
      const parsed = parseHistoryDeferredResult(m);
      recordMediaGenerationResult(parsed, afterIndex, sourceIndex);
      if (m.customType === TURN_INPUT_CONSUMPTION_EVENT_TYPE) {
        recordTurnInputConsumptionInterlude(m, afterIndex, sourceIndex);
      }
      if (m.customType === TURN_INPUT_PRESENTATION_EVENT_TYPE) {
        recordTurnInputPresentationInterlude(m, afterIndex, sourceIndex);
      }
      if (m.customType === DEFERRED_RESULT_MESSAGE_TYPE) {
        const anchor = context.deferredInterludeAnchors.get(sourceIndex);
        recordDeferredInterlude(
          parsed,
          anchor ? anchor.afterIndex : null,
          historyDeferredDeliveryId(m, sourceIndex),
          sourceIndex,
        );
      }
      if (m.customType === LOOP_TURN_MESSAGE_TYPE || m.customType === LOOP_NOTICE_MESSAGE_TYPE) {
        if (m.customType === LOOP_TURN_MESSAGE_TYPE) {
          // kickoff/wakeup 协议消息才是驱动 loop 轮的输入，但它不是合法重试目标
          // （isSessionTurnInputEntry 为 false）。指针不能停在 loop-user-prompt（custom
          // 条目，重试必报错）或 /loop 之前的真实用户消息（重试会裁掉整个循环）——
          // 置 null，让其后每个 loop 轮 assistant 回复不带 turnInputEntryId（无重试入口）。
          latestTurnInputEntryId = null;
          latestTurnInputVisible = false;
        }
        recordLoopInterlude(m, afterIndex, sourceIndex);
      }
    }
  }

  return {
    messages,
    blocks,
    mediaGenerationResults,
    standaloneMediaGenerationResults,
    recordMediaGenerationResult,
    recordDeferredInterlude,
  };
}
