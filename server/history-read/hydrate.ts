/**
 * 历史读取 —— 外部状态回灌（阶段 B / B01 同源抽取）
 *
 * /sessions/messages 主循环后段整体迁出：deferred store 终态回灌、媒体生成块解析、
 * afterIndex 重映射切片、subagent/workflow 终态回灌（含 meta/summary 缓存）、
 * session file lifecycle 块修补、registry 文件列表、todos。
 * 这段代码本就只依赖 blocks + engine 的各外部 store，搬到 hydrate 后由路由调用一次，
 * 行为不变。外部 store（deferredResults/subagentRuns/registry sidecar/session-meta/
 * agent registry）全部视为可变外部输入，只在这里实时读取，不冻结进任何目录快照。
 */
import { t } from "../../lib/i18n.ts";
import { resolveMediaGenerationBlocks } from "../block-extractors.ts";
import { buildDeferredResultRecord } from "../../lib/deferred-result-notification.ts";
import {
  materializeExecutorIdentity,
  normalizeExecutorMetadata,
  readSubagentSessionMetaSync,
} from "../../lib/subagent-executor-metadata.ts";
import { loadLatestAssistantSummaryFromSessionFile } from "../../core/message-utils.ts";
import { extractLatestTodoSnapshot, todoPanelPayloadFromSnapshot } from "../../lib/tools/todo-compat.ts";
import { browserScreenshotPath } from "../../lib/session-files/browser-screenshot-file.ts";
import { serializeSessionFile } from "../../lib/session-files/session-file-response.ts";
import { taskFromSubagentRun, mergeSubagentTaskMetadata } from "./project-page.ts";

function isTerminalDeferredTask(task) {
  return task?.status === "resolved" || task?.status === "failed" || task?.status === "aborted";
}

function currentSessionPathForId(engine, sessionId) {
  if (!sessionId) return null;
  const manifest = engine.getSessionManifest?.(sessionId) || null;
  const currentPath = manifest?.currentLocator?.path;
  return typeof currentPath === "string" && currentPath ? currentPath : null;
}

function resolveSessionCacheLocator(engine, sessionPath) {
  if (!sessionPath) return { cacheKey: null, readPath: null, sessionId: null };
  const sessionId = engine.getSessionIdForPath?.(sessionPath) || null;
  const manifest = sessionId ? engine.getSessionManifest?.(sessionId) || null : null;
  const currentPath = typeof manifest?.currentLocator?.path === "string" && manifest.currentLocator.path
    ? manifest.currentLocator.path
    : sessionPath;
  return {
    cacheKey: sessionId || sessionPath,
    readPath: currentPath,
    sessionId,
  };
}

function resolveSubagentBlockSession(engine, block, task = null, run = null) {
  const rawSessionId =
    block?.sessionId
    || task?.meta?.sessionId
    || run?.childSessionId
    || null;
  let sessionId = typeof rawSessionId === "string" && rawSessionId.trim() ? rawSessionId.trim() : null;
  let sessionPath =
    block?.streamKey
    || task?.meta?.sessionPath
    || run?.childSessionPath
    || null;
  if (typeof sessionPath !== "string" || !sessionPath.trim()) sessionPath = null;
  if (!sessionId && sessionPath) {
    sessionId = engine.getSessionIdForPath?.(sessionPath) || null;
  }
  if (sessionId) {
    sessionPath = currentSessionPathForId(engine, sessionId) || sessionPath;
  }
  return { sessionId, sessionPath };
}

// session-meta.json sidecar 按 session 目录共享；同一个 request 里遍历几十个 block
// 时不必每个 block 都重复 readFileSync + JSON.parse。调用端构造一次 Map 当 cache。
function createSubagentMetaCache(engine) {
  const map = new Map();
  return (sessionPath) => {
    if (!sessionPath) return null;
    const { cacheKey, readPath, sessionId } = resolveSessionCacheLocator(engine, sessionPath);
    if (!cacheKey || !readPath) return null;
    if (map.has(cacheKey)) return map.get(cacheKey);
    const manifestMeta = normalizeExecutorMetadata(
      engine.getSessionExecutorMetadata?.({ sessionId, sessionPath: readPath }),
    );
    const meta = manifestMeta || readSubagentSessionMetaSync(readPath);
    map.set(cacheKey, meta);
    return meta;
  };
}

function applySubagentIdentity(engine, block, task, readSessionMeta) {
  const sessionRef = resolveSubagentBlockSession(engine, block, task);
  if (sessionRef.sessionId && !block.sessionId) block.sessionId = sessionRef.sessionId;
  if (sessionRef.sessionPath) block.streamKey = sessionRef.sessionPath;
  const sessionPath = sessionRef.sessionPath;
  const sessionMeta = readSessionMeta(sessionPath);
  const resolved =
    materializeExecutorIdentity(sessionMeta, engine.getAgent?.bind(engine))
    || materializeExecutorIdentity(task?.meta, engine.getAgent?.bind(engine))
    || materializeExecutorIdentity(block, engine.getAgent?.bind(engine));

  if (resolved) {
    block.agentId = resolved.agentId;
    block.agentName = resolved.agentName;
    return;
  }

  const inferredAgentId = sessionPath
    ? engine.resolveSessionOwnership?.(sessionPath)?.agentId || null
    : null;
  if (!inferredAgentId) return;

  const inferredAgent = engine.getAgent?.(inferredAgentId) || null;
  block.agentId = inferredAgentId;
  block.agentName = inferredAgent?.agentName || "Unknown agent";
}

function patchBlockExecutorMetadata(engine, block, task, readSessionMeta) {
  const sessionRef = resolveSubagentBlockSession(engine, block, task);
  if (sessionRef.sessionId && !block.sessionId) block.sessionId = sessionRef.sessionId;
  if (sessionRef.sessionPath) block.streamKey = sessionRef.sessionPath;
  const sessionPath = sessionRef.sessionPath;
  const sessionMeta = readSessionMeta(sessionPath);
  const sources = [sessionMeta, task?.meta, block];

  for (const source of sources) {
    if (!source) continue;
    if (source.executorAgentId && !block.executorAgentId) {
      block.executorAgentId = source.executorAgentId;
    }
    if (source.executorAgentNameSnapshot && !block.executorAgentNameSnapshot) {
      block.executorAgentNameSnapshot = source.executorAgentNameSnapshot;
    }
    if (source.executorMetaVersion && !block.executorMetaVersion) {
      block.executorMetaVersion = source.executorMetaVersion;
    }
  }
}

function patchBlockRequestedMetadata(block, task = null) {
  const sources = [task?.meta, block];

  for (const source of sources) {
    if (!source) continue;
    if (source.requestedAgentId && !block.requestedAgentId) {
      block.requestedAgentId = source.requestedAgentId;
    }
    if (source.requestedAgentNameSnapshot && !block.requestedAgentName) {
      block.requestedAgentName = source.requestedAgentNameSnapshot;
    }
  }
}

function createSubagentSummaryCache(engine) {
  const map = new Map();
  return async (sessionPath) => {
    if (!sessionPath) return null;
    const { cacheKey, readPath } = resolveSessionCacheLocator(engine, sessionPath);
    if (!cacheKey || !readPath) return null;
    if (!map.has(cacheKey)) {
      map.set(cacheKey, loadLatestAssistantSummaryFromSessionFile(readPath));
    }
    return await map.get(cacheKey);
  };
}

function sessionFileLifecycleFields(file, engine) {
  const serialized = typeof engine?.serializeSessionFile === "function"
    ? engine.serializeSessionFile(file)
    : file;
  const source = serialized || file;
  const fileId = source.fileId || source.id || file.fileId || file.id || null;
  return {
    ...(fileId ? { fileId } : {}),
    ...(source.filePath ? { filePath: source.filePath } : {}),
    ...(source.label || source.displayName ? { label: source.label || source.displayName } : {}),
    ...(source.ext !== undefined ? { ext: source.ext } : {}),
    ...(source.mime ? { mime: source.mime } : {}),
    ...(source.kind ? { kind: source.kind } : {}),
    ...(source.storageKind ? { storageKind: source.storageKind } : {}),
    ...(source.presentation ? { presentation: source.presentation } : {}),
    ...(source.listed !== undefined ? { listed: source.listed !== false } : {}),
    ...(source.status ? { status: source.status } : {}),
    ...(source.missingAt !== undefined ? { missingAt: source.missingAt } : {}),
    ...(source.mtimeMs !== undefined ? { mtimeMs: source.mtimeMs } : {}),
    ...(source.size !== undefined ? { size: source.size } : {}),
    ...(source.version ? { version: source.version } : {}),
    ...(source.resource ? { resource: source.resource } : {}),
  };
}

function patchSessionFileLifecycleBlocks(blocks, engine, sessionPath) {
  if (!sessionPath) return;
  for (const block of blocks || []) {
    if (!block) continue;
    if (!["file", "artifact", "skill", "screenshot"].includes(block.type)) continue;
    let file = null;
    if (block.fileId && typeof engine?.getSessionFile === "function") {
      file = engine.getSessionFile(block.fileId, { sessionPath });
    }
    if (!file && block.filePath && typeof engine?.getSessionFileByPath === "function") {
      file = engine.getSessionFileByPath(block.filePath, { sessionPath });
    }
    if (!file && block.type === "screenshot" && block.base64 && engine?.lingxiHome && typeof engine?.getSessionFileByPath === "function") {
      try {
        const filePath = browserScreenshotPath(engine.lingxiHome, sessionPath, {
          base64: block.base64,
          mimeType: block.mimeType,
          sessionId: engine.getSessionIdForPath?.(sessionPath) || null,
        });
        file = engine.getSessionFileByPath(filePath, { sessionPath });
        if (file) block.type = "file";
      } catch {}
    }
    if (!file) continue;
    const patch = sessionFileLifecycleFields(file, engine);
    // 历史块记录展示时的内容证据；目录只更新当前位置与可用性，不能用
    // 今天的版本改写过去，也不能给缺少版本的旧记录伪造版本。
    delete patch.version;
    delete patch.size;
    delete patch.mtimeMs;
    Object.assign(block, patch);
    if (block.type === "skill" && block.installedFile) {
      block.installedFile = { ...block.installedFile, ...patch };
    }
  }
}

function listSessionRegistryFiles(engine, sessionPath, activeReferences = [], referenceIdentities = null) {
  if (!sessionPath) return [];
  // B05 热页路径（§5 #2）：目录构建期已收集的全历史引用身份集合，直接交给
  // registry.listReachable 的 referenceIdentities 入参（不再每页深扫正文）。
  // 身份集合缺失（无目录/旧引擎）时保持原全量 references 深扫路径，行为不变。
  if (referenceIdentities && typeof engine?._sessionFiles?.listReachable === "function") {
    return engine._sessionFiles.listReachable(sessionPath, undefined, { referenceIdentities })
      .map(file => {
        if (typeof engine.serializeSessionFile === "function") return engine.serializeSessionFile(file);
        return serializeSessionFile(file, { runtimeContext: engine?.runtimeContext || null });
      })
      .filter(Boolean);
  }
  if (typeof engine?.listSessionFiles !== "function") return [];
  return engine.listSessionFiles(sessionPath, { references: activeReferences })
    .map(file => {
      if (typeof engine.serializeSessionFile === "function") return engine.serializeSessionFile(file);
      return serializeSessionFile(file, { runtimeContext: engine?.runtimeContext || null });
    })
    .filter(Boolean);
}

export interface HydrateExternalStateInput {
  engine: any;
  sessionPath: string | null;
  pageBounds: { total: number; startIdx: number; endIdx: number; hasMore: boolean };
  forceAll: boolean;
  /** B01 全量模式：registry 引用来源传原 sourceMessages（原样）。 */
  activeReferences?: any[];
  /** B05 热页：目录构建期收集的全历史引用身份集合（§5 #2）；缺省走原 references 深扫路径。 */
  sessionFileReferenceIdentities?: Set<string> | null;
  /** scanner finalize 的 todoSnapshot 事实（extractLatestTodoSnapshot 的返回值）。 */
  todoSnapshot: ReturnType<typeof extractLatestTodoSnapshot>;
  blocks: any[];
  mediaGenerationResults: Map<string, any>;
  standaloneMediaGenerationResults: any[];
  recordMediaGenerationResult: (parsed: any, afterIndex: number, sourceIndex?: number | null) => void;
  recordDeferredInterlude: (parsed: any, afterIndex: number | null, deliveryId?: string | null, sourceIndex?: number | null) => void;
}

export async function hydrateExternalState(input: HydrateExternalStateInput): Promise<{
  slicedBlocks: any[];
  sessionFiles: any[];
  todos: any[] | null;
  todoPanel: ReturnType<typeof todoPanelPayloadFromSnapshot> | null;
}> {
  const {
    engine,
    sessionPath: resolvedSessionPath,
    pageBounds,
    forceAll,
    activeReferences,
    sessionFileReferenceIdentities = null,
    todoSnapshot,
    blocks,
    mediaGenerationResults,
    standaloneMediaGenerationResults,
    recordMediaGenerationResult,
    recordDeferredInterlude,
  } = input;
  const deferredStore = engine.deferredResults;

  if (resolvedSessionPath && typeof deferredStore?.listBySession === "function") {
    for (const task of deferredStore.listBySession(resolvedSessionPath)) {
      if (!isTerminalDeferredTask(task)) continue;
      const parsed = buildDeferredResultRecord(task.taskId, task);
      recordMediaGenerationResult(parsed, pageBounds.total - 1);
      recordDeferredInterlude(parsed, null);    }
  }
  const resolvedBlocks = resolveMediaGenerationBlocks(
    blocks,
    mediaGenerationResults,
    standaloneMediaGenerationResults,
  );

  // 重映射 afterIndex 到切片内偏移，过滤超出范围的
  const slicedBlocks = forceAll
    ? resolvedBlocks
    : resolvedBlocks
      .filter(b => b.afterIndex >= pageBounds.startIdx && b.afterIndex < pageBounds.endIdx)
      .map(b => ({ ...b, afterIndex: b.afterIndex - pageBounds.startIdx }));

  // 修正 subagent blocks 的状态：优先从 durable run registry 读长期映射，
  // 再用 deferred store 作为实时投递队列。deferred 会清理，不再承担历史事实源。
  {
    const deferredStore = engine.deferredResults;
    const runStore = engine.subagentRuns;
    const readSessionMeta = createSubagentMetaCache(engine);
    const readSessionSummary = createSubagentSummaryCache(engine);
    for (const b of slicedBlocks) {
      if (b.type !== "subagent" || !b.taskId) continue;
      const task = deferredStore?.query?.(b.taskId) || null;
      const run = runStore?.query?.(b.taskId) || null;
      const runTask = taskFromSubagentRun(run);
      const metadataTask = mergeSubagentTaskMetadata(runTask, task);
      const durableSessionId = run?.childSessionId || null;
      const durableSessionPath = run?.childSessionPath || null;
      const deferredSessionId = task?.meta?.sessionId || null;
      const deferredSessionPath = task?.meta?.sessionPath || null;
      if (!b.sessionId && durableSessionId) b.sessionId = durableSessionId;
      if (!b.sessionId && deferredSessionId) b.sessionId = deferredSessionId;
      if (!b.streamKey && durableSessionPath) b.streamKey = durableSessionPath;
      if (!b.streamKey && deferredSessionPath) b.streamKey = deferredSessionPath;
      {
        const sessionRef = resolveSubagentBlockSession(engine, b, metadataTask, run);
        if (sessionRef.sessionId && !b.sessionId) b.sessionId = sessionRef.sessionId;
        if (sessionRef.sessionPath) b.streamKey = sessionRef.sessionPath;
      }
      patchBlockRequestedMetadata(b, metadataTask);
      patchBlockExecutorMetadata(engine, b, metadataTask, readSessionMeta);
      applySubagentIdentity(engine, b, metadataTask, readSessionMeta);

      if (b.streamStatus !== "running") continue;

      const terminalTask = run && run.status !== "pending" ? runTask : task;

      // subagent 完成状态只能由 durable run registry 或 deferred store 的任务终态确认。
      // 子 session 可能有多轮输出，尾部 assistant 文本只能作为 resolved 后的摘要来源。
      if (terminalTask?.status === "aborted") {
        b.streamStatus = "aborted";
        b.summary = terminalTask.reason || "aborted";
        if (terminalTask.meta?.sessionPath) b.streamKey = terminalTask.meta.sessionPath;
        patchBlockRequestedMetadata(b, terminalTask);
        patchBlockExecutorMetadata(engine, b, terminalTask, readSessionMeta);
        applySubagentIdentity(engine, b, terminalTask, readSessionMeta);
        continue;
      }
      if (terminalTask?.status === "failed") {
        b.streamStatus = "failed";
        b.summary = terminalTask.reason || "failed";
        if (terminalTask.meta?.sessionPath) b.streamKey = terminalTask.meta.sessionPath;
        patchBlockRequestedMetadata(b, terminalTask);
        patchBlockExecutorMetadata(engine, b, terminalTask, readSessionMeta);
        applySubagentIdentity(engine, b, terminalTask, readSessionMeta);
        continue;
      }
      if (terminalTask?.status === "resolved") {
        b.streamStatus = "done";
        if (terminalTask.meta?.sessionPath) b.streamKey = terminalTask.meta.sessionPath;
        patchBlockRequestedMetadata(b, terminalTask);
        patchBlockExecutorMetadata(engine, b, terminalTask, readSessionMeta);
        applySubagentIdentity(engine, b, terminalTask, readSessionMeta);

        const sp = b.streamKey || terminalTask.meta?.sessionPath || null;
        const summary = await readSessionSummary(sp);
        b.summary = summary || (typeof terminalTask.result === "string" ? terminalTask.result.slice(0, 200) : b.summary);
        continue;
      }

      if (run?.status === "pending" && !task) {
        b.streamStatus = "failed";
        b.summary = t("session.subagentRunStateUnrecoverable");
        continue;
      }

      if (!b.streamKey && !run && !task) {
        b.streamStatus = "failed";
        b.summary = t("session.subagentLinkUnrecoverable");
      }
    }
  }

  // workflow inline 概览块回填：block_update patch 是前端瞬时事件、未持久化进 toolResult details，
  // 重启后块保留派单时的 streamStatus:"running" + startedAt，会显示离谱「已运行 Xm」时长。
  // 从 durable runStore 读终态修正，并用 completedAt 补 finishedAt（inline 卡算总时长用）。
  {
    const wfRunStore = engine.subagentRuns;
    const wfDeferredStore = engine.deferredResults;
    for (const b of slicedBlocks) {
      if (b.type !== "workflow" || !b.taskId) continue;
      if (b.streamStatus !== "running") continue;
      const run = wfRunStore?.query?.(b.taskId) || null;
      const task = wfDeferredStore?.query?.(b.taskId) || null;
      const status = run?.status || task?.status || null;
      if (status === "resolved" || status === "done") b.streamStatus = "done";
      else if (status === "failed") b.streamStatus = "failed";
      else if (status === "aborted") b.streamStatus = "aborted";
      else continue; // 仍 pending / 无记录：保持 running，不误判完成
      if (!b.finishedAt && run?.completedAt) {
        const ts = Date.parse(run.completedAt);
        if (Number.isFinite(ts)) b.finishedAt = ts;
      }
      if (!b.summary && typeof run?.summary === "string") b.summary = run.summary;
    }
  }

  patchSessionFileLifecycleBlocks(slicedBlocks, engine, resolvedSessionPath);
  const sessionFiles = listSessionRegistryFiles(engine, resolvedSessionPath, activeReferences, sessionFileReferenceIdentities);

  // 从历史中提取最新 todo 状态：branch-aware，沿当前 leaf 回溯到 root，
  // 只在当前分支路径上找最新合法快照。避免从抛弃的分支取到错误状态。
  // 快照合法性判定在 extractLatestTodoSnapshot（scanner finalize 已调用，这里不重扫）；
  // 输出契约与 extractLatestTodos 逐字一致：无快照 → null；removed → []；否则走 lifecycle。
  const todos = todoSnapshot
    ? (todoSnapshot.removed ? [] : todoSnapshot.todos)
    : null;
  // 面板快照（v2 收尾摘要 / 版本 / 收纳标志）随 hydrate 一起下发，
  // 前端据此恢复与实时路径一致的面板状态。
  const todoPanel = todoSnapshot ? todoPanelPayloadFromSnapshot(todoSnapshot) : null;

  return { slicedBlocks, sessionFiles, todos, todoPanel };
}
