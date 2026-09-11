/**
 * Session 管理 REST 路由
 */
import { appendFileSync } from "fs";
import fs from "fs/promises";
import path from "path";
import { randomUUID } from 'node:crypto';
import { readDesktopInputRunSnapshot } from '../../core/desktop-session-submit.ts';
import { Hono } from "hono";
import { safeJson } from "../hono-helpers.ts";
import { bodyFromRouteError, routeError, statusFromRouteError } from "./route-errors.ts";
import { t } from "../../lib/i18n.ts";
import { resolveDeferredReceiverName } from "../deferred-result-interlude.ts";
import { BrowserManager } from "../../lib/browser/browser-manager.ts";
import { isSessionJsonlFilename, sessionIdFromFilename } from "../../lib/session-jsonl.ts";
import { noteSessionFileMutation } from "../../core/session-file-mutation-epoch.ts";
import { isHiddenTurnInputMessage } from "../../lib/turn-input-presentation.ts";
import {
  extractTextContent,
  loadSessionHistoryMessages,
  loadSessionHistoryEvidence,
  isValidSessionPath,
  isActiveDesktopSessionPath,
  isArchivedDesktopSessionPath,
} from "../../core/message-utils.ts";
import { stripSessionReminderBlocks } from "../../core/session-reminders.ts";
import { sessionFileRevision } from "../../core/session-list-projection-cache.ts";
import { extractLatestTodoSnapshot, computeTodoListVersion, todoPanelPayloadFromSnapshot } from "../../lib/tools/todo-compat.ts";
import { SessionManager } from "../../lib/pi-sdk/index.ts";
import { TODO_FORMAT_VERSION, TODO_STATE_CUSTOM_TYPE } from "../../lib/tools/todo-constants.ts";
import { mergeWorkspaceHistory, normalizeWorkspacePath } from "../../shared/workspace-history.ts";
import { listStudioMountsForStudio } from "../../core/studio-mounts.ts";
import { sanitizeBridgeVisibleText } from "../../shared/bridge-visible-text.ts";
import {
  deleteSessionFileSidecarSync,
  moveSessionFileSidecarSync,
  sessionFileSidecarPath,
} from "../../lib/session-files/session-file-registry.ts";
import { getModelThinkingLevels, normalizeSessionThinkingLevel, modelSupportsXhigh, resolveModelDefaultThinkingLevel } from "../../core/session-thinking-level.ts";
import {
  modelSupportsDirectAudioInput,
  modelSupportsDirectVideoInput,
  modelSupportsAudioInput,
  modelSupportsVideoInput,
  resolveModelAudioInputTransport,
  resolveModelVideoInputTransport,
} from "../../shared/model-capabilities.ts";
import { replayLatestUserTurn, resolveSessionNodeTarget, retrySessionTurn } from "../../core/session-turn-actions.ts";
import { getWorkspaceSnapshotService } from "../../core/workspace-snapshots.ts";
import { createRequestContext } from "../http/boundary.ts";
import { createModuleLogger } from "../../lib/debug-log.ts";
import { searchSessions } from "../../lib/search/session-search.ts";
import { findInSessionMessages } from "../../lib/search/session-find.ts";
import { SessionSearchTokenizerUnavailableError } from "../../lib/search/session-search-tokenizer.ts";
import { MountAwareFileError, MountAwareFileService } from "../../core/mount-aware-file-service.ts";
import { collectToolOutcomesByCallId } from "../../shared/tool-outcome.ts";
import { resolveHistoryDeferredContent } from "../history-deferred-content.ts";
import { isDisplayableHistoryMessage } from "../history-read/projection-context.ts";
import { createSanitizeVisibleContent, isBridgeSessionPath } from "../history-read/project-page.ts";
import { resolveHistoryPageBounds } from "../history-read/page.ts";
import { evaluateHistoryConditionalGet } from "../history-read/protocol.ts";
import { readSessionHistoryOverview } from "../history-read/index.ts";
import { projectFullHistoryPage, readSessionHistoryPage } from "../history-read/index.ts";
import { HistoryDirectoryCache } from "../history-read/cache.ts";

// B01 同源抽取：页边界语义原样移入 server/history-read/page.ts，这里 re-export
// 保持既有 import 兼容（find 路由与定向测试按原路径引用）。
export { resolveHistoryPageBounds };

const log = createModuleLogger("sessions");
const lifecycleLog = createModuleLogger("sessions/lifecycle");
const switchLog = createModuleLogger("sessions/switch");
const SESSION_SEARCH_QUERY_MAX_LENGTH = 512;

function rcPlatformFromSessionKey(sessionKey) {
  const match = /^([a-z]+)_/i.exec(sessionKey || "");
  return match ? match[1] : "bridge";
}

async function pathExists(filePath) {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

// 确认剩余任务已完成：未完成项改为已完成；已取消项保持取消（不被改写）。
function completeTodoItems(todos) {
  return (Array.isArray(todos) ? todos : []).map((todo) => (
    todo?.status === "cancelled" ? todo : { ...todo, status: "completed" }
  ));
}

// 取消剩余任务：待开始/进行中/受阻项改为已取消；已完成项保持完成。
function cancelTodoItems(todos) {
  return (Array.isArray(todos) ? todos : []).map((todo) => (
    todo && TERMINAL_TODO_STATUSES.has(todo.status)
      ? todo
      : { ...todo, status: "cancelled" }
  ));
}

const TERMINAL_TODO_STATUSES = new Set(["cancelled", "completed"]);

function hasUnfinishedTodoItems(todos) {
  return (Array.isArray(todos) ? todos : []).some(
    (todo) => todo && !TERMINAL_TODO_STATUSES.has(todo.status),
  );
}

/**
 * 用户收尾操作（完成/取消）产生的新清单必然是 v2 语义：
 * 全部终态 → finished；removed/dismissed 均为 false（保留收尾摘要）。
 */
function resolveUserActionSnapshotFlags(todos) {
  const list = Array.isArray(todos) ? todos : [];
  const finished = list.length > 0 && list.every((item) => item && TERMINAL_TODO_STATUSES.has(item.status));
  return {
    removed: list.length === 0,
    dismissed: false,
    finished,
    allCompleted: finished && list.every((item) => item.status === "completed"),
    version: computeTodoListVersion(list),
  };
}

/**
 * 版本失配判定：客户端带来了版本且与服务端当前快照版本不同 → 拒绝。
 * 不带版本的旧请求沿用旧行为（作用于当前快照）。
 */
function isTodoVersionMismatch(snapshot, clientVersion) {
  return (
    typeof clientVersion === "string" &&
    clientVersion.length > 0 &&
    !!snapshot &&
    typeof snapshot.version === "string" &&
    snapshot.version !== clientVersion
  );
}

function readTodoSnapshotForManager(manager) {
  return extractLatestTodoSnapshot(manager.buildSessionContext?.().messages || []);
}

function getWritableSessionManager(engine, sessionPath) {
  const liveSession = engine.getSessionByPath?.(sessionPath);
  if (liveSession?.sessionManager) return liveSession.sessionManager;
  if (typeof engine.openSessionManagerAtCurrentBranch === "function") {
    return engine.openSessionManagerAtCurrentBranch(sessionPath, path.dirname(sessionPath));
  }
  return SessionManager.open(sessionPath, path.dirname(sessionPath));
}

function authorizeSessionRoute(requestContext, capability, target) {
  if (requestContext.authPrincipal?.kind === "unknown") return { allowed: true, reason: "legacy_test_context" };
  if (typeof requestContext.authorize !== "function") return { allowed: false, reason: "missing_policy" };
  return requestContext.authorize(capability, target);
}

function resolveSessionWorkspaceSelection(engine, requestContext, body) {
  const mountId = typeof body?.workspaceMountId === "string" && body.workspaceMountId.trim()
    ? body.workspaceMountId.trim()
    : null;
  if (!mountId) {
    return {
      cwd: typeof body?.cwd === "string" && body.cwd.trim() ? body.cwd : null,
      mount: null,
    };
  }
  if (typeof body?.cwd === "string" && body.cwd.trim()) {
    throw routeError("cwd and workspaceMountId cannot be combined", "ambiguous_workspace", 400);
  }
  try {
    const files = new MountAwareFileService({
      lingxiHome: engine.lingxiHome,
      defaultRoot: engine.defaultDeskCwd || engine.homeCwd || engine.deskCwd,
      studioId: requestContext?.studioId || engine.getRuntimeContext?.()?.studioId || null,
    });
    const root = files.resolveRoot(mountId);
    return {
      cwd: files.resolveDirectory(mountId, ""),
      mount: {
        mountId: root.mountId || root.id || mountId,
        label: root.label || null,
      },
    };
  } catch (err) {
    if (err instanceof MountAwareFileError) {
      throw routeError(err.message, err.code, err.status);
    }
    throw err;
  }
}

function sessionWorkspaceMountFields(engine, sessionPath, fallback = null) {
  const mount = engine.getSessionWorkspaceMount?.(sessionPath) || fallback || null;
  if (!mount?.mountId) return {};
  return {
    workspaceMountId: mount.mountId,
    workspaceLabel: mount.label || null,
  };
}

async function resumeBrowserForSessionSwitch(bm, sessionPath) {
  if (typeof bm.resumeForSessionIfAvailable === "function") {
    return await bm.resumeForSessionIfAvailable(sessionPath);
  }
  await bm.resumeForSession(sessionPath);
  return {
    status: "resumed",
    canResume: true,
    reason: null,
    hostConnected: null,
    hasResumeState: true,
    running: bm.isRunning(sessionPath),
    url: bm.currentUrl(sessionPath) || null,
  };
}

/**
 * 把建会话失败的异常分层成 { status, body }。
 *
 * 唯一正确的做法是在抛错点就带上 code 和 status——新增抛错点必须这么写。
 * 下面那串文案正则只服务于还没来得及带码的历史抛错点：它依赖错误信息的字面量，
 * 上游改一次文案就会失灵，翻译一变更是直接漏判。所以它是只减不增的兜底，
 * 不要再往里加语言或新的匹配分支。
 */
function classifySessionCreationError(err) {
  const message = err?.message || String(err);
  if (err?.status && Number.isInteger(err.status)) {
    return { status: err.status, body: { error: message, code: err.code || "session_create_failed" } };
  }
  if (
    /no available model/i.test(message)
    || /no available models/i.test(message)
    || /没有可用的模型/.test(message)
    || /沒有可用的模型/.test(message)
    || /利用可能なモデルがありません/.test(message)
    || /사용 가능한 모델이 없/.test(message)
  ) {
    return { status: 409, body: { error: message, code: "no_available_model" } };
  }
  return { status: 500, body: { error: message } };
}

const TODO_COMPLETE_MESSAGE =
  "[Hana Todo] The user confirmed the remaining tasks as completed. Unfinished items were marked completed; items the user previously cancelled stay cancelled. Create a new todo list only if new work needs tracking.";
const TODO_CANCEL_MESSAGE =
  "[Hana Todo] The user cancelled the remaining tasks in the current todo list. Pending, in-progress and blocked items were marked cancelled; completed items stay completed. Do not resume cancelled items unless the user explicitly asks to reopen that work.";
const TODO_DISMISS_MESSAGE =
  "[Hana Todo] The user collapsed the finished todo summary in the UI. This only changed display; task outcomes (completed/cancelled) are unchanged.";

// 与 /sessions/messages 主循环的序号语义逐字对齐：
// 只有 user/assistant 且 isDisplayableHistoryMessage 为真的消息推进 displayIdx。
// 改这里必须同步改 server/history-read/project-page.ts 的主循环
// 与 tests/session-find-route.test.ts 的一致性测试。
const FIND_LEGACY_STEER_PREFIX_RE = /^(?:（插话，无需 MOOD）|\(Interjection, no MOOD needed\))\n?/;
const FIND_TURN_TAG_PREFIX_RE = /^<t>[^<]*<\/t>\s*/;

// find 路由的热路径缓存：key 为 sessionPath，value 为 { revision, entries }。
// revision（stat 签名，与 /api/sessions 列表投影同源同格式）不一致即失效；
// revision 为 null（修订未知）时不写缓存，防止把"未知"固化成陈旧命中。
// 容量上限 FIND_ENTRIES_CACHE_MAX，超限时按插入序淘汰最早的 key。
const FIND_ENTRIES_CACHE_MAX = 8;
const findEntriesCache = new Map();

export function collectFindableHistoryEntries(sourceMessages, sanitizeVisibleContent) {
  const entries = [];
  let displayIdx = 0;
  for (const m of Array.isArray(sourceMessages) ? sourceMessages : []) {
    if (m?.role !== "user" && m?.role !== "assistant") continue;
    if (!isDisplayableHistoryMessage(m)) continue;
    const currentIndex = displayIdx;
    displayIdx += 1;
    if (m.role === "user") {
      const { text } = extractTextContent(m.content);
      const content = sanitizeVisibleContent(text);
      // 前端 history-builder 不渲染这类系统消息（history-builder.ts:503），
      // 序号照常推进，但不参与命中。
      if (isHiddenTurnInputMessage({ content })) continue;
      const visible = content
        .replace(FIND_LEGACY_STEER_PREFIX_RE, "")
        .replace(FIND_TURN_TAG_PREFIX_RE, "");
      if (visible.trim()) entries.push({ index: currentIndex, text: visible });
    } else {
      const { text } = extractTextContent(m.content, { stripThink: true });
      const content = sanitizeVisibleContent(text);
      if (content.trim()) entries.push({ index: currentIndex, text: content });
    }
  }
  return entries;
}

/**
 * 读取会话文件的磁盘修订点（stat 签名，与 /api/sessions 列表投影同源同格式）。
 * stat 失败（请求竞态中文件被归档/删除）返回 null —— 显式的「修订点未知」，
 * 前端对 null 的策略是下次触发时重新校验，不会把差异静默吞掉。
 */
async function readSessionFileRevision(sessionPath) {
  if (!sessionPath) return null;
  try {
    return sessionFileRevision(await fs.stat(sessionPath));
  } catch {
    return null;
  }
}

export function createSessionsRoute(engine, hub = null) {
  const route = new Hono();
  const lifecycleLocks = new Map();
  // B06 目录缓存：route/runtime 实例私有（非模块单例，I01）；测试可经
  // engine.historyReadCache 注入同一实例以观测统计。
  const historyDirectoryCache = engine.historyReadCache instanceof HistoryDirectoryCache
    ? engine.historyReadCache
    : new HistoryDirectoryCache();

  function getSessionSummaryRecord(sessionPath, agentIdHint = null) {
    if (!sessionPath) return null;
    const agentId = agentIdHint || engine.resolveSessionOwnership?.(sessionPath)?.agentId || null;
    if (!agentId) return null;
    const agent = engine.getAgent?.(agentId) || null;
    const summaryManager = agent?.summaryManager || null;
    if (!summaryManager || typeof summaryManager.getSummary !== "function") return null;

    const sessionId = engine.getSessionIdForPath?.(sessionPath)
      || sessionIdFromFilename(path.basename(sessionPath));
    const record = summaryManager.getSummary(sessionId);
    return record?.summary?.trim() ? record : null;
  }

  function serializeSessionSummaryRecord(record) {
    return {
      hasSummary: !!record,
      summary: record?.summary || null,
      createdAt: record?.created_at || null,
      updatedAt: record?.updated_at || null,
    };
  }

  function invalidateRcTarget(sessionPath) {
    const rcState = engine.rcState;
    if (!rcState?.invalidateDesktopSession) return;

    const { detachedAttachments } = rcState.invalidateDesktopSession(sessionPath);
    for (const attachment of detachedAttachments) {
      try {
        engine.emitEvent?.({
          type: "bridge_rc_detached",
          sessionKey: attachment.sessionKey,
          sessionPath: attachment.desktopSessionPath,
        }, attachment.desktopSessionPath);
      } catch {}
    }
  }

  function archivedPathForActiveSession(sessionPath) {
    return path.join(path.dirname(sessionPath), "archived", path.basename(sessionPath));
  }

  function activePathForArchivedSession(sessionPath) {
    return path.join(path.dirname(path.dirname(sessionPath)), path.basename(sessionPath));
  }

  /**
   * 单条「归档 transition」核心序列（锁内）：单条归档路由、工作台批量处置
   * （workspace-disposal）与孤儿清扫（sweep-orphaned-workspaces）共用。
   * 返回 { destPath, sessionId }；失败抛 routeError（状态码与原单条路由一致）。
   */
  async function archiveActiveSessionCore(engine, sessionPath, sessionId = null) {
    const destPath = archivedPathForActiveSession(sessionPath);
    return await withSessionLifecycleLock([sessionPath, destPath], async () => {
      const archiveDir = path.dirname(destPath);
      try {
        await fs.access(sessionPath);
      } catch {
        throw routeError(t("error.sessionNotFound"), "session_not_found", 404);
      }
      if (await pathExists(destPath)) {
        throw routeError("Archived path already exists", "archived_path_exists", 409);
      }
      if (await pathExists(sessionFileSidecarPath(destPath))) {
        throw routeError("Stage file sidecar destination already exists", "stage_sidecar_exists", 409);
      }
      await cleanupSessionLifecycle([sessionPath, destPath], "parent session archived", { skipMemory: true });

      await engine.setSessionPinned({
        ...(sessionId ? { sessionId } : {}),
        sessionPath,
      }, false);
      await engine.closeSession(sessionPath);

      await fs.mkdir(archiveDir, { recursive: true });
      const manifest = await moveSessionLifecycleOrThrow({
        fromPath: sessionPath,
        toPath: destPath,
        lifecycle: "archived",
        reason: "session_archive",
      });
      try {
        // C02：归档 rename 使旧路径失效——写前递增源与目标路径的变更世代。
        noteSessionFileMutation(sessionPath, "rename");
        noteSessionFileMutation(destPath, "rename");
        await fs.rename(sessionPath, destPath);
        moveSessionFileSidecarSync(sessionPath, destPath);
      } catch (err) {
        try {
          await moveSessionLifecycleOrThrow({
            fromPath: destPath,
            toPath: sessionPath,
            lifecycle: "active",
            reason: "session_archive_rollback",
          });
        } catch (rollbackErr) {
          lifecycleLog.error(`archive manifest rollback failed for ${sessionPath}: ${rollbackErr.message}`);
        }
        throw err;
      }

      // 将 mtime 置为归档瞬间，使 cleanup 按"归档时间"而非"最后活动时间"判断
      const nowSec = Date.now() / 1000;
      await fs.utimes(destPath, nowSec, nowSec);

      return { destPath, sessionId: manifest.sessionId || sessionId || null };
    });
  }

  function lifecycleLockKeyForPaths(paths) {
    for (const sessionPath of uniqueLifecyclePaths(paths)) {
      try {
        const sessionId = engine.getSessionIdForPath?.(sessionPath);
        if (typeof sessionId === "string" && sessionId.trim()) return `session:${sessionId.trim()}`;
      } catch {
        // Fall through to path-derived legacy lock keys.
      }
    }
    for (const sessionPath of uniqueLifecyclePaths(paths)) {
      const sessionPathText = typeof sessionPath === "string" ? sessionPath : "";
      const agentId = engine.resolveSessionOwnership?.(sessionPathText)?.agentId || "unknown-agent";
      const basename = path.basename(sessionPathText);
      if (basename) return `legacy:${agentId}:${basename}`;
    }
    return "legacy:unknown-session";
  }

  async function withSessionLifecycleLock(paths, fn) {
    const key = lifecycleLockKeyForPaths(paths);
    while (lifecycleLocks.has(key)) {
      await lifecycleLocks.get(key).catch(() => {});
    }
    let release;
    const held = new Promise((resolve) => { release = resolve; });
    lifecycleLocks.set(key, held);
    try {
      return await fn();
    } finally {
      if (lifecycleLocks.get(key) === held) lifecycleLocks.delete(key);
      release();
    }
  }

  async function moveSessionLifecycleOrThrow(input) {
    if (typeof engine.moveSessionLifecycle !== "function") {
      throw routeError(
        "Session manifest lifecycle transition is unavailable",
        "session_manifest_unavailable",
        503,
      );
    }
    const manifest = await engine.moveSessionLifecycle(input);
    if (!manifest?.sessionId) {
      throw routeError(
        "Session manifest lifecycle transition failed",
        "session_lifecycle_transition_failed",
        500,
      );
    }
    return manifest;
  }

  async function permanentlyDeleteArchivedFile(sessionPath, reason) {
    const stagedPath = `${sessionPath}.deleting`;
    if (await pathExists(stagedPath) || await pathExists(sessionFileSidecarPath(stagedPath))) {
      throw routeError("Archived session deletion is already staged", "session_delete_staged_conflict", 409);
    }

    // C02：永久删除先把文件 rename 到 staged——写前递增变更世代。
    noteSessionFileMutation(sessionPath, "delete");
    noteSessionFileMutation(stagedPath, "delete");
    await fs.rename(sessionPath, stagedPath);
    try {
      moveSessionFileSidecarSync(sessionPath, stagedPath);
    } catch (err) {
      await fs.rename(stagedPath, sessionPath).catch(() => {});
      throw err;
    }

    let manifest;
    try {
      manifest = await moveSessionLifecycleOrThrow({
        fromPath: sessionPath,
        toPath: sessionPath,
        lifecycle: "deleted",
        reason,
      });
    } catch (err) {
      moveSessionFileSidecarSync(stagedPath, sessionPath);
      await fs.rename(stagedPath, sessionPath).catch(() => {});
      throw err;
    }

    try {
      await fs.unlink(stagedPath);
      deleteSessionFileSidecarSync(stagedPath);
    } catch (err) {
      try {
        await moveSessionLifecycleOrThrow({
          fromPath: sessionPath,
          toPath: sessionPath,
          lifecycle: "archived",
          reason: "session_delete_rollback",
        });
        await fs.rename(stagedPath, sessionPath);
        moveSessionFileSidecarSync(stagedPath, sessionPath);
      } catch (rollbackErr) {
        lifecycleLog.error(`delete rollback failed for ${sessionPath}: ${rollbackErr.message}`);
      }
      throw err;
    }
    return manifest;
  }

  async function sessionFileHasMessages(sessionPath) {
    const raw = await fs.readFile(sessionPath, "utf-8");
    for (const line of raw.split("\n")) {
      if (!line.trim()) continue;
      let entry;
      try {
        entry = JSON.parse(line);
      } catch {
        return true;
      }
      if (entry?.type === "message" && entry.message) return true;
    }
    return false;
  }

  async function repairHeaderOnlyActiveRestoreTarget(destPath) {
    if (!(await pathExists(destPath))) return { repaired: false };
    if (await sessionFileHasMessages(destPath)) {
      throw routeError("Active path already exists with messages", "active_session_conflict", 409);
    }
    await fs.unlink(destPath);
    deleteSessionFileSidecarSync(destPath);
    return { repaired: true };
  }

  function uniqueLifecyclePaths(paths) {
    return [...new Set((paths || []).filter((p) => typeof p === "string" && p.trim()))];
  }

  function lifecycleSessionRef(sessionPath) {
    if (!sessionPath) return sessionPath;
    try {
      const sessionId = engine.getSessionIdForPath?.(sessionPath);
      if (typeof sessionId === "string" && sessionId.trim()) {
        return { sessionId: sessionId.trim(), sessionPath };
      }
    } catch {
      // Keep path-only cleanup for legacy sessions when manifest lookup fails.
    }
    return sessionPath;
  }

  async function cleanupSessionLifecycle(sessionPaths, reason, options: { skipMemory?: boolean } = {}) {
    const bm = BrowserManager.instance();
    for (const sessionPath of uniqueLifecyclePaths(sessionPaths)) {
      const sessionRef = lifecycleSessionRef(sessionPath);
      try {
        engine.taskRegistry?.abortByParentSession?.(sessionPath, reason);
      } catch (err) {
        lifecycleLog.warn(`task cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        engine.subagentRuns?.abortByParentSession?.(sessionPath, reason);
      } catch (err) {
        lifecycleLog.warn(`subagent run cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        engine.subagentThreads?.removeBySession?.(sessionPath);
      } catch (err) {
        lifecycleLog.warn(`subagent thread cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        // 右侧 workflow 卡活动随对话退场（内存 + 持久化背书一并清，按 sessionId 归属）。
        engine.activityHub?.clearBySession?.(sessionRef);
      } catch (err) {
        lifecycleLog.warn(`activity hub cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        engine.deferredResults?.suppressBySession?.(sessionRef, reason);
      } catch (err) {
        lifecycleLog.warn(`deferred cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        engine.confirmStore?.abortBySession?.(sessionRef);
      } catch (err) {
        lifecycleLog.warn(`confirm cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        if (typeof engine.discardSessionRuntime === "function") {
          if (options && Object.keys(options).length > 0) {
            await engine.discardSessionRuntime(sessionPath, reason, options);
          } else {
            await engine.discardSessionRuntime(sessionPath, reason);
          }
        } else {
          await engine.abortSessionByPath?.(sessionPath);
        }
      } catch (err) {
        lifecycleLog.warn(`session runtime cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        await bm.closeBrowserForSession(sessionPath);
      } catch (err) {
        lifecycleLog.warn(`browser cleanup failed for ${sessionPath}: ${err.message}`);
      }
      try {
        engine.terminalSessions?.closeForSession?.(sessionPath);
      } catch (err) {
        lifecycleLog.warn(`terminal cleanup failed for ${sessionPath}: ${err.message}`);
      }
      invalidateRcTarget(sessionPath);
    }
  }

  function isDeletedAgentSessionPath(sessionPath) {
    if (!sessionPath) return false;
    return engine.isDeletedAgentSession?.(sessionPath) === true;
  }

  function rejectDeletedAgentSession(c) {
    return c.json({ error: "agent_deleted", reason: "agent_deleted" }, 409);
  }

  function normalizeRequestSessionId(value) {
    return typeof value === "string" && value.trim() ? value.trim() : null;
  }

  function resolveSessionLocatorFromBody(body, operation) {
    const sessionId = normalizeRequestSessionId(body?.sessionId);
    const legacySessionPath = typeof body?.path === "string" && body.path.trim()
      ? body.path
      : typeof body?.sessionPath === "string" && body.sessionPath.trim()
        ? body.sessionPath
        : null;

    if (sessionId) {
      const manifest = engine.getSessionManifest?.(sessionId) || null;
      const sessionPath = manifest?.currentLocator?.path || null;
      if (!sessionPath) {
        throw routeError(`${operation}: session manifest not found`, "session_manifest_not_found", 404);
      }
      if (legacySessionPath && path.resolve(legacySessionPath) !== path.resolve(sessionPath)) {
        const err: any = routeError(
          `${operation}: supplied path does not match the current session locator`,
          "session_locator_mismatch",
          409,
        );
        err.sessionId = sessionId;
        err.requestedPath = legacySessionPath;
        err.currentPath = sessionPath;
        err.lifecycle = manifest.lifecycle || null;
        throw err;
      }
      return { sessionId, sessionPath, manifest };
    }

    if (!legacySessionPath) {
      throw routeError(`${operation}: sessionId or path is required`, "session_locator_required", 400);
    }
    const resolvedSessionId = normalizeRequestSessionId(engine.getSessionIdForPath?.(legacySessionPath));
    const manifest = resolvedSessionId ? engine.getSessionManifest?.(resolvedSessionId) || null : null;
    return { sessionId: resolvedSessionId, sessionPath: legacySessionPath, manifest };
  }

  function assertManifestLifecycle(ref, lifecycle, operation) {
    if (!ref?.manifest?.lifecycle) return;
    if (ref.manifest.lifecycle === lifecycle) return;
    const err: any = routeError(
      `${operation}: session lifecycle is ${ref.manifest.lifecycle}, expected ${lifecycle}`,
      "session_lifecycle_mismatch",
      409,
    );
    err.sessionId = ref.sessionId || ref.manifest.sessionId || null;
    err.currentPath = ref.manifest.currentLocator?.path || null;
    err.lifecycle = ref.manifest.lifecycle;
    throw err;
  }

  function sessionFolderScopeResponse(scope) {
    return {
      ok: true,
      sessionPath: scope?.sessionPath || null,
      cwd: scope?.cwd || null,
      workspaceFolders: Array.isArray(scope?.workspaceFolders) ? scope.workspaceFolders : [],
      authorizedFolders: Array.isArray(scope?.authorizedFolders) ? scope.authorizedFolders : [],
      sandboxFolders: Array.isArray(scope?.sandboxFolders) ? scope.sandboxFolders : [],
    };
  }

  async function validateAuthorizedFolder(rawFolder) {
    if (typeof rawFolder !== "string" || !rawFolder.trim()) {
      throw new Error("folder is required");
    }
    const folder = path.resolve(rawFolder.trim());
    let stat;
    try {
      stat = await fs.stat(folder);
    } catch {
      throw new Error("folder does not exist");
    }
    if (!stat.isDirectory()) {
      throw new Error("folder must be a directory");
    }
    return folder;
  }

  function normalizeAuthorizedFolderPath(rawFolder) {
    if (typeof rawFolder !== "string" || !rawFolder.trim()) {
      throw new Error("folder is required");
    }
    return path.resolve(rawFolder.trim());
  }

  // 列出所有 agent 的历史 session
  route.get("/sessions", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      const runtimeStudioId = requestContext.runtimeContext?.studioId || null;
      const principalStudioId = requestContext.authPrincipal?.studioId || null;
      // Same-Studio projection v0: paired clients may see the legacy session store
      // only when their authenticated Studio is the server's current Studio.
      if (runtimeStudioId && principalStudioId && runtimeStudioId !== principalStudioId) {
        return c.json({
          error: "studio_scope_mismatch",
          detail: "authenticated Studio does not match this server Studio",
        }, 403);
      }
      const sessions = await engine.listSessions();
      const attachments = engine.rcState?.listAttachments?.() || [];
      const rcAttachmentByPath = new Map(attachments.map((attachment) => [
        attachment.desktopSessionPath,
        {
          sessionKey: attachment.sessionKey,
          platform: rcPlatformFromSessionKey(attachment.sessionKey),
        },
      ]));
      return c.json(sessions.map(s => {
        const summaryRecord = getSessionSummaryRecord(s.path, s.agentId || null);
        return ({
          path: s.path,
          sessionId: s.sessionId || engine.getSessionIdForPath?.(s.path) || null,
          title: s.title || null,
          firstMessage: (s.firstMessage || "").slice(0, 100),
          modified: s.modified?.toISOString() || null,
          // 磁盘修订点（stat 签名）。web/mobile 端用它对比已缓存会话内容，
          // 决定是否补拉 /rc 接管等离线窗口写入的消息（issue #1610）。
          revision: typeof s.revision === "string" ? s.revision : null,
          messageCount: s.messageCount || 0,
          cwd: s.cwd || null,
          agentId: s.agentId || null,
          agentName: s.agentName || null,
          projectId: s.projectId || null,
          modelId: s.modelId || null,
          modelProvider: s.modelProvider || null,
          workspaceMountId: s.workspaceMountId || null,
          workspaceLabel: s.workspaceLabel || null,
          permissionMode: s.permissionMode || (typeof engine.getSessionPermissionMode === "function"
            ? engine.getSessionPermissionMode(s.path)
            : engine.permissionMode || null),
          pinnedAt: s.pinnedAt || null,
          pinOrder: Number.isFinite(s.pinOrder) ? s.pinOrder : null,
          agentDeleted: s.agentDeleted === true,
          readOnlyReason: s.readOnlyReason || (s.agentDeleted === true ? "agent_deleted" : null),
          continuationAvailable: s.continuationAvailable === true,
          deletedAt: s.deletedAt || null,
          hasSummary: !!summaryRecord,
          rcAttachment: rcAttachmentByPath.get(s.path)
            ? {
              ...(rcAttachmentByPath.get(s.path) as any),
              title: s.title || null,
            }
            : null,
          loopStatus: (() => {
            // 冷启动循环状态：只对 running/paused 注入，让会话列表常驻显示循环徽章；
            // stopped/completed 不注入（避免历史会话残留）。运行时变更靠 loop_status WS 增量。
            const rec = engine.loopController?.toolStatus(s.path);
            if (!rec || (rec.status !== "running" && rec.status !== "paused")) return null;
            return {
              status: rec.status,
              turnCount: rec.turnCount ?? 0,
              maxTurns: rec.limits?.maxTurns ?? null,
              pausedReason: rec.pausedReason ?? null,
              prompt: rec.prompt ?? null,
            };
          })(),
        });
      }));
    } catch (err) {
      return c.json({ error: err.message }, err.status || 500);
    }
  });

  route.get("/sessions/search", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      const runtimeStudioId = requestContext.runtimeContext?.studioId || null;
      const principalStudioId = requestContext.authPrincipal?.studioId || null;
      if (runtimeStudioId && principalStudioId && runtimeStudioId !== principalStudioId) {
        return c.json({
          error: "studio_scope_mismatch",
          detail: "authenticated Studio does not match this server Studio",
        }, 403);
      }

      const query = c.req.query("q") || "";
      const phase = c.req.query("phase") === "content" ? "content" : "title";
      const limit = c.req.query("limit") ? Number(c.req.query("limit")) : undefined;
      const trimmedQuery = query.trim();
      if (!trimmedQuery) return c.json({ query, phase, results: [] });
      if ([...trimmedQuery].length > SESSION_SEARCH_QUERY_MAX_LENGTH) {
        return c.json({
          error: "query_too_long",
          maxLength: SESSION_SEARCH_QUERY_MAX_LENGTH,
        }, 400);
      }

      const sessions = await engine.listSessions();
      const results = searchSessions(sessions, trimmedQuery, { phase, limit }).map((s) => ({
        path: s.path,
        sessionId: s.sessionId || engine.getSessionIdForPath?.(s.path) || null,
        title: s.title || null,
        firstMessage: (s.firstMessage || "").slice(0, 100),
        modified: s.modified?.toISOString?.() || s.modified || null,
        messageCount: s.messageCount || 0,
        cwd: s.cwd || null,
        agentId: s.agentId || null,
        agentName: s.agentName || null,
        projectId: s.projectId || null,
        modelId: s.modelId || null,
        modelProvider: s.modelProvider || null,
        workspaceMountId: s.workspaceMountId || null,
        workspaceLabel: s.workspaceLabel || null,
        pinnedAt: s.pinnedAt || null,
        pinOrder: Number.isFinite(s.pinOrder) ? s.pinOrder : null,
        agentDeleted: s.agentDeleted === true,
        readOnlyReason: s.readOnlyReason || (s.agentDeleted === true ? "agent_deleted" : null),
        continuationAvailable: s.continuationAvailable === true,
        deletedAt: s.deletedAt || null,
        matchKind: s.matchKind,
        snippet: s.snippet || "",
        score: s.score,
      }));
      return c.json({ query, phase, results });
    } catch (err) {
      if (err instanceof SessionSearchTokenizerUnavailableError) {
        log.error(`session search tokenizer unavailable: ${err.cause || err}`);
        return c.json({ error: err.message }, 503);
      }
      return c.json({ error: err.message }, 500);
    }
  });

  route.get("/sessions/find", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const querySessionId = c.req.query("sessionId") || null;
      let queryPath = c.req.query("path") || null;
      if (typeof querySessionId === "string" && querySessionId.trim()) {
        const manifest = engine.getSessionManifest?.(querySessionId.trim()) || null;
        if (!manifest?.currentLocator?.path) {
          return c.json({ error: "Session manifest not found", code: "session_manifest_not_found" }, 404);
        }
        queryPath = manifest.currentLocator.path;
      }
      // 定位语义要求显式目标：禁止回退 engine.currentSessionPath（全局焦点指针）。
      if (!queryPath) return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      if (!isValidSessionPath(queryPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath: queryPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);

      const query = (c.req.query("q") || "").trim();
      if (!query) {
        return c.json({ query, total: 0, bestIndex: null, tokens: [], matches: [], truncated: false });
      }
      if ([...query].length > SESSION_SEARCH_QUERY_MAX_LENGTH) {
        return c.json({ error: "query_too_long", maxLength: SESSION_SEARCH_QUERY_MAX_LENGTH }, 400);
      }

      // 修订点必须在读取内容之前取（同 messages 路由）：读取期间若有新写入，
      // revision 只会偏旧，下次请求会重新解析，不会把没读到的写入标成已同步。
      const revision = await readSessionFileRevision(queryPath);
      const cached = findEntriesCache.get(queryPath);
      let entries;
      if (revision && cached && cached.revision === revision) {
        entries = cached.entries;
      } else {
        const sourceMessages = await loadSessionHistoryMessages(engine, queryPath);
        const sanitize = (value) => {
          const withoutReminder = stripSessionReminderBlocks(value);
          return isBridgeSessionPath(queryPath)
            ? sanitizeBridgeVisibleText(withoutReminder)
            : withoutReminder;
        };
        entries = collectFindableHistoryEntries(sourceMessages, sanitize);
        if (revision) {
          findEntriesCache.set(queryPath, { revision, entries });
          if (findEntriesCache.size > FIND_ENTRIES_CACHE_MAX) {
            findEntriesCache.delete(findEntriesCache.keys().next().value);
          }
        }
      }
      const result = findInSessionMessages(entries, query);
      return c.json({ query, revision, ...result });
    } catch (err) {
      if (err instanceof SessionSearchTokenizerUnavailableError) {
        log.error(`session find tokenizer unavailable: ${err.cause || err}`);
        return c.json({ error: err.message }, 503);
      }
      return c.json({ error: err.message }, 500);
    }
  });

  // 获取单个 session 的滚动摘要。列表只暴露 hasSummary，正文按需读取。
  route.get("/sessions/summary", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const sessionPath = c.req.query("path") || null;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      if (!isValidSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);

      const record = getSessionSummaryRecord(sessionPath);
      return c.json(serializeSessionSummaryRecord(record));
    } catch (err) {
      return c.json({ error: err.message }, err.status || 500);
    }
  });

  // 置顶 / 取消置顶 session
  route.post("/sessions/pin", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const { pinned } = body;
      const sessionRef = resolveSessionLocatorFromBody(body, "setSessionPinned");
      const { sessionId, sessionPath } = sessionRef;
      if (typeof pinned !== "boolean") {
        return c.json({ error: t("error.missingParam", { param: "pinned" }) }, 400);
      }
      if (!isValidSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath) && pinned === true) {
        return rejectDeletedAgentSession(c);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      const { pinnedAt, pinOrder } = await engine.setSessionPinned({
        ...(sessionId ? { sessionId } : {}),
        sessionPath,
      }, pinned);
      return c.json({
        ok: true,
        pinnedAt,
        pinOrder: Number.isFinite(pinOrder) ? pinOrder : null,
        sessionId: sessionId || engine.getSessionIdForPath?.(sessionPath) || null,
      });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  // 重排置顶区：提交完整有序的 sessionId 列表，服务端整体重新编号
  route.post("/sessions/pin-order", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const rawSessionIds = Array.isArray(body?.sessionIds) ? body.sessionIds : null;
      if (!rawSessionIds || rawSessionIds.length === 0) {
        return c.json({ error: t("error.missingParam", { param: "sessionIds" }) }, 400);
      }

      const refs = [];
      const seen = new Set();
      for (const rawSessionId of rawSessionIds) {
        const sessionId = normalizeRequestSessionId(rawSessionId);
        if (!sessionId) {
          return c.json({ error: t("error.missingParam", { param: "sessionIds" }) }, 400);
        }
        if (seen.has(sessionId)) {
          return c.json({
            error: `setSessionPinOrder: duplicate session ${sessionId}`,
            code: "session_pin_order_duplicate",
            sessionId,
          }, 400);
        }
        seen.add(sessionId);
        // 每个 session 都独立解析定位并鉴权：一次请求跨多个 session，
        // 授权不能只看列表里的第一个。
        const sessionRef = resolveSessionLocatorFromBody({ sessionId }, "setSessionPinOrder");
        if (!isValidSessionPath(sessionRef.sessionPath, engine.agentsDir)) {
          return c.json({ error: "Invalid session path" }, 403);
        }
        const auth = authorizeSessionRoute(requestContext, "sessions.write", {
          kind: "session",
          studioId: requestContext.studioId,
          sessionPath: sessionRef.sessionPath,
        });
        if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
        refs.push({ sessionId: sessionRef.sessionId });
      }

      const orders = await engine.setSessionPinOrder(refs);
      return c.json({ ok: true, orders });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  /**
   * 会话记忆开关（可读/可写任意已存在的会话，不要求它是「当前会话」）。
   * 侧边对话面板会为它自己的会话读取与实际开关；服务端语义与创建会话时的
   * memoryEnabled 完全同源（manifest.memoryPolicy + session-meta）。
   */
  route.get("/sessions/memory", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const sessionPath = c.req.query("path") || null;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }
      return c.json({
        ok: true,
        sessionPath,
        memoryEnabled: engine.getSessionMemoryEnabled?.(sessionPath) !== false,
      });
    } catch (err) {
      return c.json({ error: err.message, code: err.code || undefined }, err.status || 500);
    }
  });

  route.patch("/sessions/memory", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionPath = body?.path || body?.sessionPath || null;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      if (typeof body?.memoryEnabled !== "boolean") {
        return c.json({ error: t("error.missingParam", { param: "memoryEnabled" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }
      if (typeof engine.setSessionMemoryEnabled !== "function") {
        return c.json({ error: "session memory toggle unavailable" }, 500);
      }
      const result = await engine.setSessionMemoryEnabled(sessionPath, body.memoryEnabled);
      if (result?.ok === false) {
        return c.json({ error: result.error || "failed to set session memory" }, 400);
      }
      return c.json({
        ok: true,
        sessionPath,
        memoryEnabled: result?.memoryEnabled !== false,
      });
    } catch (err) {
      return c.json({ error: err.message, code: err.code || undefined }, err.status || 500);
    }
  });

  route.get("/sessions/authorized-folders", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const sessionPath = c.req.query("path") || engine.currentSessionPath || null;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }
      return c.json(sessionFolderScopeResponse(engine.getSessionFolderScope?.(sessionPath)));
    } catch (err) {
      return c.json({ error: err.message, code: err.code || undefined }, err.status || 500);
    }
  });

  route.patch("/sessions/authorized-folders", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionPath = body?.path || body?.sessionPath || null;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }

      const action = typeof body?.action === "string" ? body.action.trim() : "set";
      let scope;
      if (action === "add") {
        const folder = await validateAuthorizedFolder(body?.folder);
        scope = await engine.addSessionAuthorizedFolder?.(sessionPath, folder);
      } else if (action === "remove") {
        const folder = normalizeAuthorizedFolderPath(body?.folder);
        scope = await engine.removeSessionAuthorizedFolder?.(sessionPath, folder);
      } else if (action === "set") {
        const folders = Array.isArray(body?.folders) ? body.folders : [];
        const normalizedFolders = [];
        for (const folder of folders) {
          normalizedFolders.push(await validateAuthorizedFolder(folder));
        }
        scope = await engine.setSessionAuthorizedFolders?.(sessionPath, normalizedFolders);
      } else {
        return c.json({ error: "Invalid action" }, 400);
      }
      return c.json(sessionFolderScopeResponse(scope || engine.getSessionFolderScope?.(sessionPath)));
    } catch (err) {
      const message = err.message || String(err);
      if (/folder (is required|does not exist|must be a directory)/.test(message)) {
        return c.json({ error: message }, 400);
      }
      return c.json({ error: message }, 500);
    }
  });

  // 历史重内容只在用户真正展开时读取。凭证只定位当前会话中的原始条目，
  // 仍需经过与消息列表相同的路径校验和读取授权，不能把它当成文件路径使用。
  route.get("/sessions/content/:contentId", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const querySessionId = c.req.query("sessionId") || null;
      let queryPath = c.req.query("path") || null;
      if (typeof querySessionId === "string" && querySessionId.trim()) {
        const manifest = engine.getSessionManifest?.(querySessionId.trim()) || null;
        if (!manifest?.currentLocator?.path) {
          return c.json({ error: "Session manifest not found", code: "session_manifest_not_found" }, 404);
        }
        queryPath = manifest.currentLocator.path;
      }
      if (queryPath && !isValidSessionPath(queryPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const resolvedSessionPath = queryPath || engine.currentSessionPath || null;
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath: resolvedSessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!resolvedSessionPath) return c.json({ error: "Session not found" }, 404);

      const sourceMessages = await loadSessionHistoryMessages(engine, resolvedSessionPath);
      const resolved = resolveHistoryDeferredContent(
        sourceMessages,
        c.req.param("contentId"),
      );
      return resolved
        ? c.json(resolved)
        : c.json({ error: "Historical content not found" }, 404);
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  // 会话冻结的提示词快照 + 工具名列表（session-meta.json sidecar；只读展示面用，
  // 模型观测轨迹详情的 SYSTEM 首记录在未开启载荷捕获时从这里取系统提示词）。
  route.get("/sessions/prompt-snapshot", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const querySessionId = (c.req.query("sessionId") || "").trim();
      const queryPath = c.req.query("path") || null;
      let sessionPath: string | null = queryPath;
      if (querySessionId) {
        const manifest = engine.getSessionManifest?.(querySessionId) || null;
        if (!manifest?.currentLocator?.path) {
          return c.json({ error: "Session manifest not found", code: "session_manifest_not_found" }, 404);
        }
        sessionPath = manifest.currentLocator.path;
      }
      if (sessionPath && !isValidSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath: sessionPath || engine.currentSessionPath || null,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      const resolvedSessionPath = sessionPath || engine.currentSessionPath || null;
      if (!resolvedSessionPath) return c.json({ error: "Session path not resolved" }, 404);
      const context = await engine.sessionCoordinator.readSessionPromptContextByPath(resolvedSessionPath);
      return c.json({
        promptSnapshot: context.promptSnapshot,
        toolNames: context.toolNames,
      });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  // 获取 session 的消息（支持 ?path= 指定 session，否则读焦点 session）
  route.get("/sessions/history-overview", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      // 身份优先级与 /sessions/messages 逐字对齐：sessionId → manifest locator → path。
      const querySessionId = c.req.query("sessionId") || null;
      let queryPath = c.req.query("path") || null;
      if (typeof querySessionId === "string" && querySessionId.trim()) {
        const manifest = engine.getSessionManifest?.(querySessionId.trim()) || null;
        if (!manifest?.currentLocator?.path) {
          return c.json({ error: "Session manifest not found", code: "session_manifest_not_found" }, 404);
        }
        queryPath = manifest.currentLocator.path;
      }
      if (queryPath && !isValidSessionPath(queryPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const resolvedSessionPath = queryPath || engine.currentSessionPath || null;
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath: resolvedSessionPath || engine.currentSessionPath || null,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      // "只是统计"不是公开数据：复用 sessions.read；概览不新增任何执行/详情功能（E04.2）。
      const result = await readSessionHistoryOverview({
        engine,
        cache: historyDirectoryCache,
        sessionPath: resolvedSessionPath,
        sessionId: querySessionId?.trim() || (resolvedSessionPath ? engine.getSessionIdForPath?.(resolvedSessionPath) || null : null),
        studioId: requestContext.studioId ?? null,
        disableCache: engine.historyReadDisableCache === true,
      });
      // E04 响应头按协议合同（无 ETag：条件快路径仅限普通消息页）。
      const headers = {
        "lingxi-history-protocol": "1",
        "cache-control": "private, no-store",
      };
      if (result.kind === "unavailable") {
        return c.body(JSON.stringify({ schemaVersion: 1, available: false, reason: result.reason }), 200, headers);
      }
      return c.body(JSON.stringify(result.overview), 200, headers);
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  route.get("/sessions/messages", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const querySessionId = c.req.query("sessionId") || null;
      let queryPath = c.req.query("path") || null;
      if (typeof querySessionId === "string" && querySessionId.trim()) {
        const manifest = engine.getSessionManifest?.(querySessionId.trim()) || null;
        if (!manifest?.currentLocator?.path) {
          return c.json({ error: "Session manifest not found", code: "session_manifest_not_found" }, 404);
        }
        queryPath = manifest.currentLocator.path;
      }
      if (queryPath && !isValidSessionPath(queryPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath: queryPath || engine.currentSessionPath || null,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      const resolvedSessionPath = queryPath || engine.currentSessionPath || null;
      const reconciling = c.req.query('reconciliation') === '1';
      const reconciliationSessionId = querySessionId || engine.getSessionIdForPath?.(resolvedSessionPath) || null;
      const beforeRun = reconciling ? readDesktopInputRunSnapshot(engine, reconciliationSessionId, resolvedSessionPath) : null;
      // 修订点必须在读取内容之前取：读取期间若有新写入，revision 只会偏旧
      // （前端下次触发时多补拉一次，方向安全），不会偏新（把没读到的写入
      // 标成「已同步」会让 /rc 消息永久漏掉，issue #1610 的反方向竞态）。
      const revision = await readSessionFileRevision(resolvedSessionPath);
      const sanitizeVisibleContent = createSanitizeVisibleContent(resolvedSessionPath);

      // 分页参数
      const beforeId = c.req.query("before") != null ? Number(c.req.query("before")) : null;
      const limit = Math.min(Number(c.req.query("limit")) || 50, 200);

      // all=1 强制全量返回（流式恢复等特殊场景）
      const forceAll = c.req.query("all") === "1";

      if (reconciling) {
        // reconciliation=1：严格证据等级（I12）——完全不进目录/条件快路径（B07），
        // 与普通分页共用同一全量 projector（I04）。
        const evidence = await loadSessionHistoryEvidence(engine, resolvedSessionPath, reconciliationSessionId);
        const result = await projectFullHistoryPage(engine, {
          sessionPath: resolvedSessionPath,
          sourceMessages: evidence.messages,
          beforeId,
          limit,
          forceAll,
          sanitizeVisibleContent,
        });
        const afterRun = reconciling ? readDesktopInputRunSnapshot(engine, reconciliationSessionId, resolvedSessionPath) : null;
        const reconciliation = reconciling ? {
          sessionId: reconciliationSessionId, sessionPath: resolvedSessionPath, snapshotId: randomUUID(),
          runRevision: afterRun!.revision, complete: evidence!.complete,
          runStatus: !evidence!.complete || beforeRun!.revision !== afterRun!.revision ? 'unknown'
            : beforeRun!.status === 'running' || afterRun!.status === 'running' ? 'running'
              : beforeRun!.status === 'reconciled_idle' && afterRun!.status === 'reconciled_idle' ? 'reconciled_idle' : 'unknown',
          ...(evidence!.diagnostic ? { diagnostic: evidence!.diagnostic } : {}),
        } : undefined;
        return c.json({
          messages: result.messages, blocks: result.blocks, todos: result.todos,
          todoPanel: result.todoPanel ?? null,
          hasMore: result.hasMore, nextBefore: result.nextBefore, sessionFiles: result.sessionFiles,
          revision, ...(reconciliation ? { reconciliation } : {}),
        });
      }

      // 普通分页：目录快路径（B06 生产启用点）。身份解析/授权已先行（I01）；
      // 快照/回退链内建：目录尝试 1 → invalidate+重建 → 尝试 2 → legacy 全量（P11）。
      const outcome = await readSessionHistoryPage({
        engine,
        cache: historyDirectoryCache,
        sessionPath: resolvedSessionPath,
        sessionId: reconciliationSessionId,
        studioId: requestContext.studioId ?? null,
        beforeId,
        limit,
        forceAll,
        sanitizeVisibleContent,
        disableCache: engine.historyReadDisableCache === true,
      });
      if (outcome.mode === "error") throw outcome.error;

      // 重启后右侧 workflow 卡复原：ActivityHub 已从持久化背书回灌该会话的 workflow 活动，
      // 这里在「首屏载入」（非翻页）时重发一遍，让前端 agent-activity slice 重新填充。
      // 翻页（beforeId != null）不重发，避免重复广播；目录重试成功后恰一次（B07）。
      if (beforeId == null && resolvedSessionPath) {
        engine.activityHub?.rebroadcastSession?.(resolvedSessionPath);
      }

      // E02 条件 GET：成功返回边界（授权/B-C 读取/快照复核/外部状态补齐/rebroadcast
      // 全部完成后）。序列化当前页面一次：200 复用同一字节串发送；字节串不写入任何
      // 缓存，请求结束即释放（E02.1）。all/reconciliation 已在上方分支返回，不进入
      // 本段（E01 304 资格）。
      // COMPAT：旧客户端不发 If-None-Match → 恒走无条件 200 分支；条件求值与协议头
      // 实现见 server/history-read/protocol.ts；相关测试
      // tests/history-protocol-conditional.test.ts；退役条件=全部连接确认协议 v1
      // 能力后无能力回退分支才可移除（不设自动删除日期）。
      const responseBody = JSON.stringify({
        messages: outcome.result.messages, blocks: outcome.result.blocks, todos: outcome.result.todos,
        todoPanel: outcome.result.todoPanel ?? null,
        hasMore: outcome.result.hasMore, nextBefore: outcome.result.nextBefore, sessionFiles: outcome.result.sessionFiles,
        revision,
      });
      const conditional = evaluateHistoryConditionalGet({
        responseBodyUtf8: responseBody,
        revision,
        forceAll,
        reconciling,
        mode: outcome.mode,
        scope: {
          principalId: requestContext.principalId ?? null,
          serverNodeId: requestContext.serverNodeId ?? null,
          studioId: requestContext.studioId ?? null,
          sessionId: reconciliationSessionId,
          normalizedPath: resolvedSessionPath ? path.resolve(resolvedSessionPath) : null,
          branchIdentity: outcome.branchIdentity ?? null,
        },
        beforeId,
        limit,
        ifNoneMatch: c.req.header("If-None-Match"),
        requestId: c.req.header("x-request-id") ?? null,
      });
      if (conditional.status === 304) {
        return c.body(null, 304, conditional.headers);
      }
      return c.body(responseBody, 200, conditional.headers);
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  route.post("/sessions/latest-user-message/replay", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionPath = body?.path || body?.sessionPath || null;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }
      if (engine.isSessionStreaming?.(sessionPath)) {
        return c.json({ error: "session_busy" }, 409);
      }

      const result = await replayLatestUserTurn(engine, {
        sessionPath,
        sourceEntryId: body.sourceEntryId || null,
        clientMessageId: body.clientMessageId || null,
        snapshotVersion: body.snapshotVersion,
        replacementText: typeof body.text === "string" ? body.text : undefined,
        displayMessage: body.displayMessage || null,
        uiContext: body.uiContext ?? null,
      });
      return c.json({ ok: true, ...result });
    } catch (err) {
      const status = err.message === "session_busy" ? 409 : 400;
      return c.json({ error: err.message }, status);
    }
  });

  route.post("/sessions/turns/retry", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionRef = resolveSessionLocatorFromBody(body, "retrySessionTurn");
      assertManifestLifecycle(sessionRef, "active", "retrySessionTurn");
      const { sessionId, sessionPath } = sessionRef;
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }
      if (engine.isSessionStreaming?.(sessionPath)) {
        return c.json({ error: "session_busy" }, 409);
      }

      // fileRollback：默认 none（保持现状行为）；workspace 必须显式开启开关，
      // 未开启时显式 4xx，绝不静默降级成 none。
      const rawFileRollback = body?.fileRollback;
      if (rawFileRollback != null && rawFileRollback !== "none" && rawFileRollback !== "workspace") {
        return c.json({ error: "invalid fileRollback", code: "invalid_file_rollback" }, 400);
      }
      const fileRollback = rawFileRollback === "workspace" ? "workspace" : "none";
      if (fileRollback === "workspace" && engine.preferences?.getRollbackFileChanges?.() !== true) {
        return c.json({
          error: "file rollback is disabled in preferences",
          code: "file_rollback_disabled",
        }, 403);
      }

      const result = await retrySessionTurn(engine, {
        sessionId,
        sessionPath,
        target: body?.target,
        clientMessageId: body?.clientMessageId || null,
        snapshotVersion: body?.snapshotVersion,
        replacementText: typeof body?.text === "string" ? body.text : undefined,
        displayMessage: body?.displayMessage || null,
        uiContext: body?.uiContext ?? null,
        ...(rawFileRollback != null ? { fileRollback } : {}),
      });
      return c.json({ ok: true, ...result });
    } catch (err) {
      // 无码默认 400（重放失败绝大多数是请求本身的问题），session_busy 仍单独回 409。
      return c.json(
        bodyFromRouteError(err),
        statusFromRouteError(err, err?.message === "session_busy" ? 409 : 400),
      );
    }
  });

  /**
   * 「回退时撤销文件改动」预览：开关关闭 / 无检查点 / 拍照降级都在这里显式说明，
   * UI 据此决定选项置灰与影响文件数，而不是先请求再失败。
   */
  route.post("/sessions/turns/rollback-preview", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionRef = resolveSessionLocatorFromBody(body, "previewWorkspaceRollback");
      assertManifestLifecycle(sessionRef, "active", "previewWorkspaceRollback");
      const { sessionId, sessionPath } = sessionRef;
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }

      const enabled = engine.preferences?.getRollbackFileChanges?.() === true;
      if (!enabled) {
        return c.json({
          ok: true,
          enabled: false,
          available: false,
          degraded: false,
          commit: null,
          reason: "file_rollback_disabled",
          fileCount: 0,
          files: [],
        });
      }
      if (typeof engine.ensureSessionLoaded !== "function") {
        return c.json({
          ok: true,
          enabled: true,
          available: false,
          degraded: false,
          commit: null,
          reason: "session_branch_unavailable",
          fileCount: 0,
          files: [],
        });
      }
      const session = await engine.ensureSessionLoaded(sessionPath);
      const branch = session?.sessionManager?.getBranch?.() || [];
      const target = body?.target
        || (typeof body?.turnInputEntryId === "string" ? { role: "user", entryId: body.turnInputEntryId } : null);
      const resolved = resolveSessionNodeTarget(branch, target, { mode: "retry" });
      const turnInputEntryId = resolved.turnInputEntry.id;
      const ownerAgentId = engine.resolveSessionOwnership?.(sessionPath)?.agentId
        || engine.getSessionManifest?.(sessionId)?.ownerAgentId
        || null;
      const workspaceRoot = ownerAgentId
        ? (engine.getExplicitHomeCwd?.(ownerAgentId) || engine.getHomeCwd?.(ownerAgentId) || null)
        : null;
      if (!workspaceRoot) {
        return c.json({
          ok: true,
          enabled: true,
          available: false,
          degraded: false,
          commit: null,
          reason: "workspace_unavailable",
          turnInputEntryId,
          fileCount: 0,
          files: [],
        });
      }
      const service = getWorkspaceSnapshotService({ lingxiHome: engine.lingxiHome });
      const preview = await service.previewTurn({
        sessionPath,
        workspaceRoot,
        turnInputEntryId,
        createdAtHint: resolved.turnInputEntry?.timestamp ?? null,
      });
      return c.json({ ok: true, enabled: true, turnInputEntryId, ...preview });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err, 400));
    }
  });

  /** 读取「回退时撤销文件改动」开关（全局偏好，默认关闭）。 */
  route.get("/sessions/workspace-rollback", async (c) => {
    try {
      return c.json({ enabled: engine.preferences?.getRollbackFileChanges?.() === true });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  /** 写入「回退时撤销文件改动」开关。 */
  route.put("/sessions/workspace-rollback", async (c) => {
    try {
      const body = await safeJson(c);
      if (typeof body?.enabled !== "boolean") {
        return c.json({ error: "enabled must be a boolean" }, 400);
      }
      if (typeof engine.preferences?.setRollbackFileChanges !== "function") {
        return c.json({ error: "preference unavailable" }, 503);
      }
      const enabled = engine.preferences.setRollbackFileChanges(body.enabled);
      return c.json({ ok: true, enabled });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  route.post("/sessions/fork", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionRef = resolveSessionLocatorFromBody(body, "forkSessionAtNode");
      assertManifestLifecycle(sessionRef, "active", "forkSessionAtNode");
      const { sessionId, sessionPath } = sessionRef;
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: "session not found" }, 404);
      }
      if (engine.isSessionStreaming?.(sessionPath)) {
        return c.json({ error: "session_busy" }, 409);
      }
      if (typeof engine.forkSessionAtNode !== "function") {
        throw routeError("session fork is unavailable", "session_fork_unavailable", 503);
      }

      const result = await engine.forkSessionAtNode({
        sessionId,
        sessionPath,
        target: body?.target,
      });
      const childPath = result?.sessionPath || result?.path || null;
      const childSessionId = result?.sessionId || (childPath ? engine.getSessionIdForPath?.(childPath) : null) || null;
      const permissionMode = result?.permissionMode || engine.getSessionPermissionMode?.(childPath) || "ask";
      const response = {
        ok: true,
        path: childPath,
        sessionPath: childPath,
        sessionId: childSessionId,
        agentId: result?.agentId || null,
        agentName: result?.agentName || engine.getAgent?.(result?.agentId)?.agentName || result?.agentId || null,
        cwd: result?.cwd || null,
        workspaceFolders: Array.isArray(result?.workspaceFolders) ? result.workspaceFolders : [],
        authorizedFolders: Array.isArray(result?.authorizedFolders) ? result.authorizedFolders : [],
        planMode: result?.planMode ?? permissionMode === "read_only",
        permissionMode,
        accessMode: result?.accessMode || (permissionMode === "read_only" ? "read_only" : "operate"),
        thinkingLevel: normalizeSessionThinkingLevel(result?.thinkingLevel),
        projectId: result?.projectId ?? null,
        workspaceMountId: result?.workspaceMountId || null,
        workspaceLabel: result?.workspaceLabel || null,
        sourceSessionId: result?.sourceSessionId || sessionId,
        forkedFromEntryId: result?.forkedFromEntryId || null,
        target: result?.target || body?.target || null,
        sessionFiles: result?.sessionFiles || { files: [], refs: [], fileIdMap: {} },
        visionNotes: result?.visionNotes || { notes: 0, keys: [] },
        memoryModelUnavailableReason: engine.memoryModelUnavailableReason || null,
      };
      hub?.eventBus?.emit?.({
        type: "session_created",
        session: response,
      }, childPath);
      return c.json(response);
    } catch (err) {
      // 无码默认 500（fork 失败多半是文件系统或引擎侧的问题），session_busy 仍单独回 409。
      return c.json(
        bodyFromRouteError(err),
        statusFromRouteError(err, err?.message === "session_busy" ? 409 : 500),
      );
    }
  });

  route.post("/sessions/todos/complete", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionPath = body?.path;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      try {
        await fs.access(sessionPath);
      } catch {
        return c.json({ error: t("error.sessionNotFound") }, 404);
      }
      // 模型正在输出时服务端同样拒绝收尾操作（前端禁用只是第一道）。
      if (engine.isSessionStreaming?.(sessionPath)) {
        return c.json({ error: "Cannot complete todos while session is streaming" }, 409);
      }

      const manager = getWritableSessionManager(engine, sessionPath);
      const snapshot = readTodoSnapshotForManager(manager);
      // 版本失配：用户操作针对的不是当前这一版清单，拒绝并提示刷新（A17）。
      if (isTodoVersionMismatch(snapshot, body?.version)) {
        return c.json({ error: t("error.todoVersionMismatch"), code: "todo_version_mismatch" }, 409);
      }
      let panel;
      if (!snapshot?.removed && hasUnfinishedTodoItems(snapshot?.todos)) {
        const completedTodos = completeTodoItems(snapshot.todos);
        manager.appendCustomMessageEntry(
          TODO_STATE_CUSTOM_TYPE,
          TODO_COMPLETE_MESSAGE,
          false,
          {
            action: "complete_all",
            source: "user",
            todoVersion: TODO_FORMAT_VERSION,
            removed: false,
            dismissed: false,
            todos: completedTodos,
          },
        );
        engine.syncSessionBranchHead?.(sessionPath, manager, "todo_complete_append");
        panel = todoPanelPayloadFromSnapshot({
          ...resolveUserActionSnapshotFlags(completedTodos),
          todos: completedTodos,
        });
      } else {
        panel = todoPanelPayloadFromSnapshot(snapshot);
      }

      engine.emitEvent?.({ type: "todo_update", ...panel }, sessionPath);
      return c.json({ ok: true, panel });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  route.post("/sessions/todos/cancel", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionPath = body?.path;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      try {
        await fs.access(sessionPath);
      } catch {
        return c.json({ error: t("error.sessionNotFound") }, 404);
      }
      // 只改变这份计划；不终止终端进程、工作流或其他后台任务。
      if (engine.isSessionStreaming?.(sessionPath)) {
        return c.json({ error: "Cannot cancel todos while session is streaming" }, 409);
      }

      const manager = getWritableSessionManager(engine, sessionPath);
      const snapshot = readTodoSnapshotForManager(manager);
      if (isTodoVersionMismatch(snapshot, body?.version)) {
        return c.json({ error: t("error.todoVersionMismatch"), code: "todo_version_mismatch" }, 409);
      }
      let panel;
      if (!snapshot?.removed && hasUnfinishedTodoItems(snapshot?.todos)) {
        const cancelledTodos = cancelTodoItems(snapshot.todos);
        manager.appendCustomMessageEntry(
          TODO_STATE_CUSTOM_TYPE,
          TODO_CANCEL_MESSAGE,
          false,
          {
            action: "cancel_remaining",
            source: "user",
            todoVersion: TODO_FORMAT_VERSION,
            removed: false,
            dismissed: false,
            todos: cancelledTodos,
          },
        );
        engine.syncSessionBranchHead?.(sessionPath, manager, "todo_cancel_append");
        panel = todoPanelPayloadFromSnapshot({
          ...resolveUserActionSnapshotFlags(cancelledTodos),
          todos: cancelledTodos,
        });
      } else {
        panel = todoPanelPayloadFromSnapshot(snapshot);
      }

      engine.emitEvent?.({ type: "todo_update", ...panel }, sessionPath);
      return c.json({ ok: true, panel });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  route.post("/sessions/todos/dismiss", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionPath = body?.path;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      try {
        await fs.access(sessionPath);
      } catch {
        return c.json({ error: t("error.sessionNotFound") }, 404);
      }

      const manager = getWritableSessionManager(engine, sessionPath);
      const snapshot = readTodoSnapshotForManager(manager);
      if (isTodoVersionMismatch(snapshot, body?.version)) {
        return c.json({ error: t("error.todoVersionMismatch"), code: "todo_version_mismatch" }, 409);
      }
      // 只收纳"已结束且未收纳"的清单；其他情况是幂等 no-op。
      // 收纳只改变当前展示，不改完成/取消结果；历史记录保留。
      if (snapshot?.finished && !snapshot.dismissed && !snapshot.removed) {
        manager.appendCustomMessageEntry(
          TODO_STATE_CUSTOM_TYPE,
          TODO_DISMISS_MESSAGE,
          false,
          {
            action: "dismiss",
            source: "user",
            todoVersion: TODO_FORMAT_VERSION,
            removed: true,
            dismissed: true,
            todos: snapshot.todos,
          },
        );
        engine.syncSessionBranchHead?.(sessionPath, manager, "todo_dismiss_append");
      }

      const panel = { removed: true, dismissed: true, todos: [] };
      engine.emitEvent?.({ type: "todo_update", ...panel }, sessionPath);
      return c.json({ ok: true, panel });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  // 新建 session（可选指定工作目录和 agentId）
  route.post("/sessions/new", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      const body = await safeJson(c);
      const { memoryEnabled, agentId, currentSessionPath: oldSessionPath, thinkingLevel } = body;
      const workspaceSelection = resolveSessionWorkspaceSelection(engine, requestContext, body);
      const cwd = workspaceSelection.cwd;
      const workspaceFolders = Array.isArray(body.workspaceFolders)
        ? body.workspaceFolders.filter(p => typeof p === "string" && p.trim())
        : [];
      const projectId = Object.prototype.hasOwnProperty.call(body, "projectId")
        ? (
            typeof engine.normalizeSessionProjectAssignmentId === "function"
              ? engine.normalizeSessionProjectAssignmentId(body.projectId)
              : (typeof body.projectId === "string" && body.projectId.trim() ? body.projectId.trim() : null)
          )
        : null;
      const memFlag = memoryEnabled !== false; // 默认 true
      log.log(`新建 session ${JSON.stringify({
        hasCwd: !!cwd,
        memoryEnabled: memFlag,
        customAgent: !!agentId,
      })}`);

      // 新建前挂起浏览器（保存当前 session 的浏览器状态）
      const bm = BrowserManager.instance();
      if (oldSessionPath && bm.isRunning(oldSessionPath)) {
        await bm.suspendForSession(oldSessionPath);
      }

      const createOptions: {
        workspaceFolders: any;
        visibleInSessionList: boolean;
        thinkingLevel?: any;
        workspaceMountId?: string;
        workspaceLabel?: string | null;
      } = { workspaceFolders, visibleInSessionList: true };
      if (thinkingLevel !== undefined && thinkingLevel !== null) {
        createOptions.thinkingLevel = thinkingLevel;
      }
      if (workspaceSelection.mount?.mountId) {
        createOptions.workspaceMountId = workspaceSelection.mount.mountId;
        createOptions.workspaceLabel = workspaceSelection.mount.label || null;
      }
      let newSessionPath, newSessionId, newAgentId;
      // @ui-focus-ok: this asks "is the caller switching to a different agent
      // than the one already on screen?", which is a question about the focus
      // itself. The caller's own view wins when it says so; the server's focus
      // is the last resort for deciding whether this is a switch at all, and
      // the agent being switched to is the explicit one either way.
      if (agentId && agentId !== (body.currentAgentId || engine.currentAgentId)) {
        ({ sessionPath: newSessionPath, sessionId: newSessionId, agentId: newAgentId } = await engine.createSessionForAgent(
          agentId,
          cwd || undefined,
          memFlag,
          undefined,
          createOptions,
        ));
      } else {
        ({ sessionPath: newSessionPath, sessionId: newSessionId, agentId: newAgentId } = await engine.createSession(
          null,
          cwd || undefined,
          memFlag,
          undefined,
          createOptions,
        ));
      }
      engine.persistSessionMeta(newSessionPath);
      if (projectId && typeof engine.setSessionProjectAssignment === "function") {
        await engine.setSessionProjectAssignment({ sessionPath: newSessionPath, projectId });
      }

      // 记住工作目录 + 更新历史
      if (cwd) {
        const history = mergeWorkspaceHistory(engine.config.cwd_history, [cwd]);
        await engine.updateConfig({ last_cwd: cwd, cwd_history: history });
      }

      log.log("session 创建完成");
      const response = {
        ok: true,
        path: newSessionPath,
        sessionId: newSessionId || engine.getSessionIdForPath?.(newSessionPath) || null,
        cwd: engine.cwd,
        workspaceFolders: engine.getSessionWorkspaceFolders?.(newSessionPath) || [],
        authorizedFolders: engine.getSessionAuthorizedFolders?.(newSessionPath) || [],
        agentId: newAgentId,
        agentName: engine.getAgent(newAgentId)?.agentName || engine.agentName,
        projectId,
        planMode: engine.planMode,
        permissionMode: engine.permissionMode,
        accessMode: engine.accessMode,
        thinkingLevel: normalizeSessionThinkingLevel(engine.getSessionThinkingLevel?.(newSessionPath) || engine.getThinkingLevel?.()),
        memoryModelUnavailableReason: engine.memoryModelUnavailableReason || null,
        ...sessionWorkspaceMountFields(engine, newSessionPath, workspaceSelection.mount),
      };
      hub?.eventBus?.emit?.({
        type: "session_created",
        session: response,
      }, newSessionPath);
      return c.json(response);
    } catch (err) {
      const classified = classifySessionCreationError(err);
      return c.json(classified.body, classified.status);
    }
  });

  route.post("/sessions/new-detached", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (typeof engine.createDetachedSession !== "function") {
        return c.json({ error: "detached session creation unavailable" }, 500);
      }

      const body = await safeJson(c);
      const { memoryEnabled, agentId, permissionMode, thinkingLevel } = body;
      const workspaceSelection = resolveSessionWorkspaceSelection(engine, requestContext, body);
      const cwd = workspaceSelection.cwd;
      const workspaceFolders = Array.isArray(body.workspaceFolders)
        ? body.workspaceFolders.filter(p => typeof p === "string" && p.trim())
        : [];
      const memFlag = memoryEnabled !== false;
      const projectId = Object.prototype.hasOwnProperty.call(body, "projectId")
        ? (
            typeof engine.normalizeSessionProjectAssignmentId === "function"
              ? engine.normalizeSessionProjectAssignmentId(body.projectId)
              : (typeof body.projectId === "string" && body.projectId.trim() ? body.projectId.trim() : null)
          )
        : null;

      const detachedOptions: {
        cwd: any;
        memoryEnabled: boolean;
        agentId: string | null;
        workspaceFolders: any;
        visibleInSessionList: boolean;
        permissionMode: any;
        thinkingLevel?: any;
        workspaceMountId?: string;
        workspaceLabel?: string | null;
      } = {
        cwd: cwd || undefined,
        memoryEnabled: memFlag,
        agentId: typeof agentId === "string" && agentId.trim() ? agentId.trim() : null,
        workspaceFolders,
        visibleInSessionList: true,
        permissionMode: permissionMode || null,
      };
      if (thinkingLevel !== undefined && thinkingLevel !== null) {
        detachedOptions.thinkingLevel = thinkingLevel;
      }
      if (workspaceSelection.mount?.mountId) {
        detachedOptions.workspaceMountId = workspaceSelection.mount.mountId;
        detachedOptions.workspaceLabel = workspaceSelection.mount.label || null;
      }

      const result = await engine.createDetachedSession(detachedOptions);
      const newSessionPath = result.sessionPath;
      const newAgentId = result.agentId;
      const newSessionId = result.sessionId || engine.getSessionIdForPath?.(newSessionPath) || null;
      engine.persistSessionMeta?.(newSessionPath);
      if (projectId && typeof engine.setSessionProjectAssignment === "function") {
        await engine.setSessionProjectAssignment({ sessionPath: newSessionPath, projectId });
      }
      if (cwd && body.recordWorkspaceHistory === true) {
        const history = mergeWorkspaceHistory(engine.config?.cwd_history, [cwd]);
        await engine.updateConfig?.({ last_cwd: cwd, cwd_history: history });
      }

      const resolvedPermissionMode = engine.getSessionPermissionMode?.(newSessionPath)
        || permissionMode
        || engine.permissionMode
        || "ask";
      const response = {
        ok: true,
        path: newSessionPath,
        sessionId: newSessionId,
        cwd: result.session?.sessionManager?.getCwd?.() || cwd || engine.cwd || null,
        workspaceFolders: engine.getSessionWorkspaceFolders?.(newSessionPath) || workspaceFolders,
        authorizedFolders: engine.getSessionAuthorizedFolders?.(newSessionPath) || [],
        agentId: newAgentId,
        agentName: engine.getAgent?.(newAgentId)?.agentName || newAgentId || engine.agentName,
        projectId,
        currentSessionPath: engine.currentSessionPath || null,
        planMode: resolvedPermissionMode === "read_only",
        permissionMode: resolvedPermissionMode,
        accessMode: resolvedPermissionMode === "read_only" ? "read_only" : "operate",
        thinkingLevel: normalizeSessionThinkingLevel(engine.getSessionThinkingLevel?.(newSessionPath) || engine.getThinkingLevel?.()),
        memoryModelUnavailableReason: engine.memoryModelUnavailableReason || null,
        ...sessionWorkspaceMountFields(engine, newSessionPath, workspaceSelection.mount),
      };
      hub?.eventBus?.emit?.({
        type: "session_created",
        session: response,
      }, newSessionPath);
      return c.json(response);
    } catch (err) {
      const classified = classifySessionCreationError(err);
      return c.json(classified.body, classified.status);
    }
  });

  route.post("/sessions/continue-deleted-agent", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const body = await safeJson(c);
      const sessionPath = body?.path || body?.sessionPath || null;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (!isDeletedAgentSessionPath(sessionPath)) {
        return c.json({ error: "agent_not_deleted" }, 400);
      }
      if (!(await pathExists(sessionPath))) {
        return c.json({ error: t("error.sessionNotFound") }, 404);
      }

      const result = await engine.continueDeletedAgentSession(sessionPath);
      const newSessionPath = result.sessionPath;
      const newAgentId = result.agentId;
      const response = {
        ok: true,
        path: newSessionPath,
        sessionId: result.sessionId || engine.getSessionIdForPath?.(newSessionPath) || null,
        cwd: result.cwd || engine.cwd || null,
        workspaceFolders: result.workspaceFolders || engine.getSessionWorkspaceFolders?.(newSessionPath) || [],
        authorizedFolders: result.authorizedFolders || engine.getSessionAuthorizedFolders?.(newSessionPath) || [],
        agentId: newAgentId,
        agentName: result.agentName || engine.getAgent?.(newAgentId)?.agentName || newAgentId,
        projectId: null,
        planMode: engine.planMode,
        permissionMode: engine.permissionMode,
        accessMode: engine.accessMode,
        thinkingLevel: normalizeSessionThinkingLevel(engine.getSessionThinkingLevel?.(newSessionPath) || engine.getThinkingLevel?.()),
        memoryModelUnavailableReason: engine.memoryModelUnavailableReason || null,
        compacted: result.compacted === true,
        compactionError: result.compactionError || null,
      };
      hub?.eventBus?.emit?.({
        type: "session_created",
        session: response,
      }, newSessionPath);
      return c.json(response);
    } catch (err) {
      const status = Number.isInteger(err?.status) && err.status >= 400 && err.status < 600
        ? err.status
        : 500;
      return c.json({
        error: err.message,
        ...(err?.code ? { code: err.code } : {}),
      }, status);
    }
  });

  // 切换 session（支持跨 agent）
  route.post("/sessions/switch", async (c) => {
    try {
      const body = await safeJson(c);
      const { sessionId, path: legacySessionPath, currentSessionPath: oldSessionPath } = body;
      let sessionPath = typeof legacySessionPath === "string" ? legacySessionPath : null;
      if (typeof sessionId === "string" && sessionId.trim()) {
        const manifest = engine.getSessionManifest?.(sessionId.trim()) || null;
        if (!manifest?.currentLocator?.path) {
          return c.json({ error: "Session manifest not found", code: "session_manifest_not_found" }, 404);
        }
        sessionPath = manifest.currentLocator.path;
      }
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "sessionId" }) }, 400);
      }
      // 运行路径只允许 active desktop session。归档会话必须先 restore。
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      // 切换前挂起浏览器（保存当前 session 的浏览器状态）
      const bm = BrowserManager.instance();
      const suspendPath = oldSessionPath;
      if (suspendPath && bm.isRunning(suspendPath)) {
        // viewer 开着就让它跟着切，不再因为切换会话把窗口藏起来
        await bm.suspendForSession(suspendPath, { keepViewerVisible: true });
      }

      await engine.switchSession(sessionPath);

      // 恢复目标 session 的浏览器（若有）。无 browser host 的 server/PWA 环境只记录 typed skip；
      // 一旦判断为可恢复，resumeForSession 内的真实 browser 错误仍会向外抛出。
      const browserResume = await resumeBrowserForSessionSwitch(bm, sessionPath);

      const session = engine.getSessionByPath(sessionPath);

      // viewer 跟随：无论是否 resume 成功都告知 viewer 当前 session（没有标签页组就显示空态）。
      // 不改变窗口可见性，viewer 没开着时这条通知只更新标题缓存。
      void bm.notifyViewerSession(sessionPath, session?.title || null);

      // 从 manifest 归属解析 agentId，避免依赖 engine 焦点指针的时序。
      // switchSession 只接受 agents/{id}/sessions/*.jsonl 布局的路径，归属要么
      // 来自 manifest，要么从这个布局推出来，所以走到这里一定解析得到 agentId。
      const switchedAgentId = engine.resolveSessionOwnership(sessionPath).agentId;
      const switchedAgent = engine.getAgent(switchedAgentId);

      // switchSession 已同步设置焦点到目标 session。
      // cwd/planMode/model 是 session 级状态，此时读焦点是安全的。
      // memoryEnabled 需要返回 session 自身冻结下来的值，而不是当前
      // master && session 的临时组合态；否则现有 session 的缓存前缀身份
      // 会被全局 gate 混淆。
      // agentId/agentName 已从 sessionPath 解析，不依赖焦点。
      const activeModel = session?.model ?? engine.currentModel;
      const modelAvailability = engine.getSessionModelAvailability?.(sessionPath) || null;
      const frozenSessionMemoryEnabled = typeof engine.getSessionMemoryEnabled === "function"
        ? engine.getSessionMemoryEnabled(sessionPath)
        : (switchedAgent?.isSessionMemoryEnabledFor?.(sessionPath) ?? engine.memoryEnabled);
      return c.json({
        ok: true,
        messageCount: session?.messages?.length || 0,
        memoryEnabled: frozenSessionMemoryEnabled,
        planMode: engine.planMode,
        permissionMode: engine.permissionMode,
        accessMode: engine.accessMode,
        thinkingLevel: normalizeSessionThinkingLevel(engine.getSessionThinkingLevel?.(sessionPath) || engine.getThinkingLevel?.()),
        memoryModelUnavailableReason: engine.memoryModelUnavailableReason || null,
        cwd: engine.cwd,
        workspaceFolders: engine.getSessionWorkspaceFolders?.(sessionPath) || [],
        authorizedFolders: engine.getSessionAuthorizedFolders?.(sessionPath) || [],
        ...sessionWorkspaceMountFields(engine, sessionPath),
        agentId: switchedAgentId,
        agentName: switchedAgent?.agentName || switchedAgentId,
        browserRunning: bm.isRunning(sessionPath),
        browserUrl: bm.currentUrl(sessionPath) || null,
        browserResume,
        isStreaming: engine.isSessionStreaming(sessionPath),
        currentModelId: activeModel?.id || null,
        currentModelProvider: activeModel?.provider || null,
        currentModelName: activeModel?.name || null,
        currentModelInput: Array.isArray(activeModel?.input) ? activeModel.input : null,
        currentModelVideo: modelSupportsVideoInput(activeModel),
        currentModelVideoTransport: resolveModelVideoInputTransport(activeModel),
        currentModelVideoTransportSupported: modelSupportsDirectVideoInput(activeModel),
        currentModelAudio: modelSupportsAudioInput(activeModel),
        currentModelAudioTransport: resolveModelAudioInputTransport(activeModel),
        currentModelAudioTransportSupported: modelSupportsDirectAudioInput(activeModel),
        currentModelReasoning: activeModel?.reasoning ?? null,
        currentModelXhigh: modelSupportsXhigh(activeModel),
        currentModelThinkingLevels: activeModel ? getModelThinkingLevels(activeModel) : null,
        currentModelDefaultThinkingLevel: activeModel ? resolveModelDefaultThinkingLevel(activeModel) : null,
        currentModelContextWindow: activeModel?.contextWindow ?? null,
        currentModelAvailable: modelAvailability?.available !== false,
        currentModelUnavailableReason: modelAvailability?.available === false
          ? (modelAvailability.reason || "temporarily_unavailable")
          : null,
      });
    } catch (err) {
      const errDetail = `${err.message}\n${err.stack || ""}`;
      switchLog.error(`error: ${errDetail}`);
      try { appendFileSync(path.join(engine.lingxiHome, "switch-error.log"), `${new Date().toISOString()}\n${errDetail}\n---\n`); } catch {}
      // 日志保持全量，响应按下游语义分层：session-coordinator 抛的 409/404/503
      // 不该在这里被压平成 500，否则用户只看得到"未知错误"，无从判断该刷新还是重试。
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  // 显式更新 Agent 能力：fresh compact 压缩旧对话，再用当前配置重建 prompt/工具快照。
  route.post("/sessions/fresh-compact", async (c) => {
    try {
      const body = await safeJson(c);
      const { path: sessionPath } = body || {};
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      const result = await engine.freshCompactDesktopSession(sessionPath);
      return c.json({
        ok: true,
        ...result,
      });
    } catch (err) {
      lifecycleLog.error(`fresh-compact failed: ${err.message}`);
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  // 获取所有有浏览器的 session
  route.get("/browser/sessions", async (c) => {
    const bm = BrowserManager.instance();
    return c.json(bm.getBrowserSessions());
  });

  // 获取所有有浏览器痕迹的 session 状态（活跃 / 可恢复 / 不可用）
  route.get("/browser/session-states", async (c) => {
    const bm = BrowserManager.instance();
    return c.json(bm.getBrowserSessionStates());
  });

  // 打开指定 session 的浏览器（侧栏徽章左键入口）：冷状态先恢复，再让 viewer 展示该 session
  route.post("/browser/open-session", async (c) => {
    const body = await safeJson(c);
    const { sessionPath } = body;
    if (!sessionPath) return c.json({ error: "missing sessionPath" }, 400);
    const bm = BrowserManager.instance();
    const resume = await bm.resumeForSessionIfAvailable(sessionPath);
    const session = engine.getSessionByPath(sessionPath);
    await bm.notifyViewerSession(sessionPath, session?.title || null);
    return c.json({ ok: true, resume });
  });

  // 关闭指定 session 的浏览器
  route.post("/browser/close-session", async (c) => {
    const body = await safeJson(c);
    const { sessionPath, revoke } = body;
    if (!sessionPath) return c.json({ error: "missing sessionPath" });
    const bm = BrowserManager.instance();
    // 急停（revoke）不只是关窗，还要撤销该 session 的 agent 浏览器授权。
    if (revoke === true) bm.revokeBrowserAuthorization(sessionPath);
    await bm.closeBrowserForSession(sessionPath);
    hub?.eventBus?.emit?.({ type: "browser_status", running: false, url: null }, sessionPath);
    return c.json({ ok: true, sessions: bm.getBrowserSessionStates() });
  });

  // 重命名 session
  route.post("/sessions/rename", async (c) => {
    try {
      const body = await safeJson(c);
      const { path: sessionPath, title } = body;
      if (!sessionPath) {
        return c.json({ error: t("error.missingParam", { param: "path" }) }, 400);
      }
      if (typeof title !== "string" || !title.trim()) {
        return c.json({ error: t("error.missingParam", { param: "title" }) }, 400);
      }
      if (!isValidSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      if (isDeletedAgentSessionPath(sessionPath)) {
        return rejectDeletedAgentSession(c);
      }
      await engine.saveSessionTitle(sessionPath, title.trim());
      return c.json({ ok: true });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  // 清理过期归档 session
  route.post("/sessions/cleanup", async (c) => {
    try {
      const body = await safeJson(c);
      const { maxAgeDays = 90 } = body;
      const cutoff = Date.now() - maxAgeDays * 86400000;
      let deleted = 0;

      // 遍历所有 agent 的 sessions/archived/ 目录
      const agentsDir = engine.agentsDir;
      const agents = await fs.readdir(agentsDir).catch(() => []);
      for (const agentId of agents) {
        const archiveDir = path.join(agentsDir, agentId, "sessions", "archived");
        let files;
        try { files = await fs.readdir(archiveDir); } catch { continue; }
        for (const f of files) {
          if (!isSessionJsonlFilename(f)) continue;
          const fp = path.join(archiveDir, f);
          try {
            const stat = await fs.stat(fp);
            if (stat.mtime.getTime() < cutoff) {
              const activeKey = path.join(agentsDir, agentId, "sessions", f);
              await cleanupSessionLifecycle([activeKey, fp], "parent session deleted");
              await permanentlyDeleteArchivedFile(fp, "session_cleanup");
              deleted++;
              // 清理 titles.json 孤儿（key = 对应的活跃路径）
              try { await engine.clearSessionTitle(activeKey); } catch {}
            }
          } catch {}
        }
      }

      return c.json({ ok: true, deleted, maxAgeDays });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  // 列出所有已归档 session（聚合各 agent 的 archived/ 目录）
  route.get("/sessions/archived", async (c) => {
    try {
      const list = await engine.listArchivedSessions();
      // firstMessage 截断与 /api/sessions 列表投影保持一致（100 字符）。
      return c.json((Array.isArray(list) ? list : []).map((s: any) => ({
        ...s,
        firstMessage: (s?.firstMessage || "").slice(0, 100),
      })));
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  // 归档 session（支持跨 agent）
  route.post("/sessions/archive", async (c) => {
    try {
      const body = await safeJson(c);
      const sessionRef = resolveSessionLocatorFromBody(body, "archiveSession");
      assertManifestLifecycle(sessionRef, "active", "archiveSession");
      const { sessionId, sessionPath } = sessionRef;
      // archive 是 lifecycle transition，只允许 active desktop session。
      if (!isActiveDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const archivedPath = await archiveActiveSessionCore(engine, sessionPath, sessionId);
      return c.json({ ok: true, sessionId: archivedPath.sessionId, archivedPath: archivedPath.destPath });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  // 工作台处置：移除工作台前，把它名下的全部会话归档或永久删除。
  // 身份口径与左栏作用域一致：mount 严格匹配 + 目录路径双形态（经挂载创建的带
  // workspaceMountId；历史 cwd 形态的老会话按 native 根路径并入），不漏任何一本账。
  route.post("/sessions/workspace-disposal", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);

      const body = await safeJson(c);
      const action = body?.action === "delete" ? "delete" : "archive";
      const mountId = typeof body?.workspaceMountId === "string" && body.workspaceMountId.trim()
        ? body.workspaceMountId.trim()
        : null;
      const cwdIdentity = typeof body?.cwd === "string" && body.cwd.trim()
        ? normalizeWorkspacePath(body.cwd.trim())
        : null;
      if (!mountId && !cwdIdentity) {
        return c.json({ error: "missing workspace identity", code: "missing_workspace_identity" }, 400);
      }
      // mount 身份：解析出 native 根路径用于 cwd 双形态匹配（此时工作台尚未移除，可解析）。
      let mountRootPath = cwdIdentity;
      if (mountId) {
        try {
          const selection = resolveSessionWorkspaceSelection(engine, requestContext, { workspaceMountId: mountId });
          mountRootPath = normalizeWorkspacePath(selection.cwd) || cwdIdentity;
        } catch (err) {
          return c.json(bodyFromRouteError(err), statusFromRouteError(err));
        }
      }

      const sessions = Array.isArray(await engine.listSessions()) ? await engine.listSessions() : [];
      const mountRootNormalized = mountRootPath ? normalizeWorkspacePath(mountRootPath) : null;
      const targets = sessions.filter((s) => {
        const sessionMountId = typeof s?.workspaceMountId === "string" ? s.workspaceMountId.trim() : "";
        if (mountId && sessionMountId) return sessionMountId === mountId;
        if (mountRootNormalized && typeof s?.cwd === "string" && s.cwd.trim()) {
          const sessionCwd = normalizeWorkspacePath(s.cwd);
          return !!sessionCwd && sessionCwd === mountRootNormalized;
        }
        return false;
      });

      let disposed = 0;
      let skippedStreaming = 0;
      const errors: Array<{ path: string; message: string }> = [];
      for (const target of targets) {
        if (engine.isSessionStreaming?.(target.path)) {
          skippedStreaming += 1;
          continue;
        }
        try {
          const { destPath: archivedPath } = await archiveActiveSessionCore(engine, target.path, target.sessionId || null);
          if (action === "delete") {
            const activeKey = activePathForArchivedSession(archivedPath);
            await withSessionLifecycleLock([activeKey, archivedPath], async () => {
              await cleanupSessionLifecycle([activeKey, archivedPath], "parent session deleted");
              const draftSessionId = target.sessionId || engine.getSessionIdForPath?.(activeKey) || null;
              try {
                await permanentlyDeleteArchivedFile(archivedPath, "archived_session_deleted");
              } catch (err) {
                if (err?.code === "ENOENT") throw routeError("session not found", "session_not_found", 404);
                throw err;
              }
              if (draftSessionId) {
                try { engine.deleteSessionInputDrafts?.(draftSessionId); } catch { /* 草稿清理失败不阻塞删除 */ }
              }
              try { await engine.clearSessionTitle(activeKey); } catch {}
            });
          }
          disposed += 1;
        } catch (err) {
          errors.push({ path: target.path, message: err?.message || String(err) });
        }
      }
      return c.json({
        ok: true,
        action,
        matched: targets.length,
        disposed,
        skippedStreaming,
        errors,
      });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  // 孤儿会话清扫：所属工作台已不存在（mount 被移除 / 磁盘目录被直接删除）的会话
  // 静默自动归档——不弹框（用户裁决：归档界面按工作台分组后可见、可整组处理）。
  // 边界：目录仍在磁盘上但未被引用的 cwd 会话不清扫（换配置目录的残留可经重新
  // 打开该目录找回，不算暗数据）。
  route.post("/sessions/sweep-orphaned-workspaces", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);

      const studioId = requestContext?.studioId || engine.getRuntimeContext?.()?.studioId || null;
      const activeMountIds = new Set(["default"]);
      try {
        for (const mount of listStudioMountsForStudio(engine.lingxiHome, studioId)) {
          if (mount.status === "active" && mount.mountId) activeMountIds.add(mount.mountId);
        }
      } catch { /* mount 列表读取失败按仅 default 处理，宁可不清扫也不误归档 */ }

      const sessions = Array.isArray(await engine.listSessions()) ? await engine.listSessions() : [];
      const orphans = [];
      for (const s of sessions) {
        const sessionMountId = typeof s?.workspaceMountId === "string" ? s.workspaceMountId.trim() : "";
        if (sessionMountId) {
          if (!activeMountIds.has(sessionMountId)) orphans.push(s);
          continue;
        }
        const cwd = typeof s?.cwd === "string" ? s.cwd.trim() : "";
        if (cwd && !(await pathExists(cwd))) orphans.push(s);
      }

      let archived = 0;
      let skippedStreaming = 0;
      const errors: Array<{ path: string; message: string }> = [];
      for (const target of orphans) {
        if (engine.isSessionStreaming?.(target.path)) {
          skippedStreaming += 1;
          continue;
        }
        try {
          await archiveActiveSessionCore(engine, target.path, target.sessionId || null);
          archived += 1;
        } catch (err) {
          errors.push({ path: target.path, message: err?.message || String(err) });
        }
      }
      return c.json({ ok: true, scanned: sessions.length, archived, skippedStreaming, errors });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  // 恢复归档 session → 移回 sessions/
  route.post("/sessions/restore", async (c) => {
    try {
      const body = await safeJson(c);
      const sessionRef = resolveSessionLocatorFromBody(body, "restoreSession");
      assertManifestLifecycle(sessionRef, "archived", "restoreSession");
      const { sessionId, sessionPath } = sessionRef;
      if (!isArchivedDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      // 必须位于 /archived/ 目录下，防止把活跃 session 当归档路径调用
      const archDir = path.dirname(sessionPath);
      if (path.basename(archDir) !== "archived") {
        return c.json({ error: "Not an archived session path" }, 403);
      }
      try {
        await fs.access(sessionPath);
      } catch {
        return c.json({ error: t("error.sessionNotFound") }, 404);
      }

      const activeDir = path.dirname(archDir);
      const destPath = path.join(activeDir, path.basename(sessionPath));

      return await withSessionLifecycleLock([destPath, sessionPath], async () => {
        try {
          await fs.access(sessionPath);
        } catch {
          return c.json({ error: t("error.sessionNotFound") }, 404);
        }

        await cleanupSessionLifecycle([destPath, sessionPath], "parent session restored", { skipMemory: true });

        // 冲突检测：目标位置已有真实消息时，不自动合并；只有旧 bug 留下的 header-only 文件可修复。
        await repairHeaderOnlyActiveRestoreTarget(destPath);
        if (await pathExists(sessionFileSidecarPath(destPath))) {
          return c.json({ error: "Stage file sidecar destination already exists" }, 409);
        }

        // C02：恢复 rename——写前递增源与目标路径的变更世代。
        noteSessionFileMutation(sessionPath, "rename");
        noteSessionFileMutation(destPath, "rename");
        await fs.rename(sessionPath, destPath);
        moveSessionFileSidecarSync(sessionPath, destPath);
        let manifest = null;
        try {
          manifest = await moveSessionLifecycleOrThrow({
            fromPath: sessionPath,
            toPath: destPath,
            lifecycle: "active",
            reason: "session_restore",
          });
        } catch (err) {
          try {
            await fs.mkdir(archDir, { recursive: true });
            await fs.rename(destPath, sessionPath);
            moveSessionFileSidecarSync(destPath, sessionPath);
          } catch (rollbackErr) {
            lifecycleLog.error(`restore file rollback failed for ${destPath}: ${rollbackErr.message}`);
          }
          throw err;
        }
        return c.json({ ok: true, restoredPath: destPath, sessionId: manifest?.sessionId || sessionId || null });
      });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  // 永久删除一条归档 session
  route.post("/sessions/archived/delete", async (c) => {
    try {
      const body = await safeJson(c);
      const sessionRef = resolveSessionLocatorFromBody(body, "deleteArchivedSession");
      assertManifestLifecycle(sessionRef, "archived", "deleteArchivedSession");
      const { sessionId, sessionPath } = sessionRef;
      if (!isArchivedDesktopSessionPath(sessionPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const archDir = path.dirname(sessionPath);
      if (path.basename(archDir) !== "archived") {
        return c.json({ error: "Not an archived session path" }, 403);
      }
      const activeKey = activePathForArchivedSession(sessionPath);
      return await withSessionLifecycleLock([activeKey, sessionPath], async () => {
        const draftSessionId = sessionId || engine.getSessionIdForPath?.(activeKey) || null;
        await cleanupSessionLifecycle([activeKey, sessionPath], "parent session deleted");
        let deletedManifest;
        try {
          deletedManifest = await permanentlyDeleteArchivedFile(sessionPath, "archived_session_deleted");
        } catch (err) {
          if (err.code === "ENOENT") {
            return c.json({ error: t("error.sessionNotFound") }, 404);
          }
          throw err;
        }
        if (draftSessionId) {
          try { engine.deleteSessionInputDrafts?.(draftSessionId); } catch { /* 草稿清理失败不阻塞删除 */ }
        }
        // 清理 titles.json 孤儿（key = 对应的活跃路径）
        try { await engine.clearSessionTitle(activeKey); } catch {}
        return c.json({ ok: true, sessionId: deletedManifest?.sessionId || sessionId || null });
      });
    } catch (err) {
      return c.json(bodyFromRouteError(err), statusFromRouteError(err));
    }
  });

  return route;
}

// 仅供测试使用的内部函数出口；生产调用一律走 route handler。
// 这个出口跟 classifySessionCreationError 的文案正则兜底同生共死：它存在的唯一理由
// 是让那段兜底可被直接测到。兜底删除之日，这个出口一并删除，不要往里加第二个成员。
export const __testables = { classifySessionCreationError };
