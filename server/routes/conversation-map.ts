/**
 * 会话地图（conversation map）REST 路由
 *
 * 原生复刻 dsh-synapse 的服务端能力：
 * - GET /conversation-map/turns  把会话历史折叠成「一问一答」的回合卡片数据
 * - GET/PUT /conversation-map/layout  画布布局（节点坐标/折叠态）的持久化，
 *   存放在 lingxiHome/conversation-map.json，原子写入。
 */
import { Hono } from "hono";
import fs from "fs/promises";
import path from "path";
import { createRequestContext } from "../http/boundary.ts";
import {
  extractTextContent,
  isValidSessionPath,
  loadSessionHistoryMessages,
} from "../../core/message-utils.ts";

/** 与 sessions.ts 同语义：unknown principal 视为旧测试上下文放行。 */
function authorizeSessionRoute(requestContext, capability, target) {
  if (requestContext.authPrincipal?.kind === "unknown") return { allowed: true, reason: "legacy_test_context" };
  if (typeof requestContext.authorize !== "function") return { allowed: false, reason: "missing_policy" };
  return requestContext.authorize(capability, target);
}

const DEFAULT_LAYOUT = { version: 1, positions: {}, collapsed: [], updatedAt: null };
const TURN_TEXT_LIMIT = 8000;
const POSITION_KEY_LIMIT = 200;
const POSITION_COORD_LIMIT = 100000;
const MAX_POSITION_ENTRIES = 5000;
const MAX_COLLAPSED_ENTRIES = 5000;

function layoutFilePath(engine) {
  return path.join(engine.lingxiHome, "conversation-map.json");
}

/** 防御性归一化磁盘上的布局文件；任何字段非法都回退到默认形状。 */
function normalizeLayout(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return { ...DEFAULT_LAYOUT };
  const positions = {};
  if (value.positions && typeof value.positions === "object" && !Array.isArray(value.positions)) {
    for (const [key, position] of Object.entries(value.positions)) {
      if (!position || typeof position !== "object") continue;
      const x = (position as any).x;
      const y = (position as any).y;
      if (typeof x !== "number" || typeof y !== "number" || !Number.isFinite(x) || !Number.isFinite(y)) continue;
      positions[key] = { x, y };
    }
  }
  const collapsed = Array.isArray(value.collapsed)
    ? value.collapsed.filter((item) => typeof item === "string")
    : [];
  return {
    version: 1,
    positions,
    collapsed,
    updatedAt: typeof value.updatedAt === "string" ? value.updatedAt : null,
  };
}

async function readLayoutFile(engine) {
  try {
    const raw = await fs.readFile(layoutFilePath(engine), "utf-8");
    return normalizeLayout(JSON.parse(raw));
  } catch {
    return { ...DEFAULT_LAYOUT };
  }
}

/** 归一化 PUT 进来的 positions：键长限制、坐标取整并夹取，超量按先来先得丢弃。 */
function sanitizePositionsInput(value) {
  const positions = {};
  if (!value || typeof value !== "object" || Array.isArray(value)) return positions;
  for (const [key, position] of Object.entries(value)) {
    if (Object.keys(positions).length >= MAX_POSITION_ENTRIES) break;
    if (typeof key !== "string" || !key || key.length > POSITION_KEY_LIMIT) continue;
    if (!position || typeof position !== "object") continue;
    const x = (position as any).x;
    const y = (position as any).y;
    if (typeof x !== "number" || typeof y !== "number" || !Number.isFinite(x) || !Number.isFinite(y)) continue;
    const clamp = (n) => Math.min(POSITION_COORD_LIMIT, Math.max(-POSITION_COORD_LIMIT, Math.round(n)));
    positions[key] = { x: clamp(x), y: clamp(y) };
  }
  return positions;
}

function sanitizeCollapsedInput(value) {
  if (!Array.isArray(value)) return [];
  const collapsed = [];
  for (const item of value) {
    if (collapsed.length >= MAX_COLLAPSED_ENTRIES) break;
    if (typeof item !== "string" || !item || item.length > POSITION_KEY_LIMIT) continue;
    collapsed.push(item);
  }
  return collapsed;
}

/** 把展示消息折叠成回合：一条 user 开启新回合，直到下一条 user 之前的 assistant/toolResult 都归入该回合。 */
function foldMessagesIntoTurns(messages) {
  const turns = [];
  let current = null;
  for (const m of messages || []) {
    if (!m || m.display === false) continue;
    if (m.role === "custom") continue;
    if (m.role === "user") {
      const { text } = extractTextContent(m.content, { stripThink: true });
      current = {
        turnIndex: turns.length,
        questionEntryId: m.id || null,
        question: (text || "").trim(),
        questionAt: m.timestamp || null,
        answerEntryId: null,
        answer: "",
        answerAt: null,
        processCount: 0,
        entryIds: m.id ? [m.id] : [],
      };
      turns.push(current);
      continue;
    }
    if (m.role === "assistant") {
      if (!current) {
        current = {
          turnIndex: turns.length,
          questionEntryId: null,
          question: "",
          questionAt: null,
          answerEntryId: null,
          answer: "",
          answerAt: null,
          processCount: 0,
          entryIds: [],
        };
        turns.push(current);
      }
      const { text, toolUses } = extractTextContent(m.content, { stripThink: true });
      if ((text || "").trim()) {
        current.answer = text.trim();
        current.answerEntryId = m.id || null;
        current.answerAt = m.timestamp || null;
      }
      current.processCount += (toolUses || []).length;
      if (m.id) current.entryIds.push(m.id);
      continue;
    }
    if (m.role === "toolResult") {
      if (current) {
        current.processCount += 1;
        if (m.id) current.entryIds.push(m.id);
      }
      continue;
    }
  }
  for (const turn of turns) {
    let truncated = false;
    if (turn.question.length > TURN_TEXT_LIMIT) {
      turn.question = turn.question.slice(0, TURN_TEXT_LIMIT);
      truncated = true;
    }
    if (turn.answer.length > TURN_TEXT_LIMIT) {
      turn.answer = turn.answer.slice(0, TURN_TEXT_LIMIT);
      truncated = true;
    }
    turn.truncated = truncated;
  }
  return turns;
}

export function createConversationMapRoute(engine) {
  const route = new Hono();

  route.get("/conversation-map/turns", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const querySessionId = c.req.query("sessionId") || null;
      let resolvedPath = c.req.query("path") || null;
      if (typeof querySessionId === "string" && querySessionId.trim()) {
        const manifest = engine.getSessionManifest?.(querySessionId.trim()) || null;
        if (!manifest?.currentLocator?.path) {
          return c.json({ error: "Session manifest not found", code: "session_manifest_not_found" }, 404);
        }
        resolvedPath = manifest.currentLocator.path;
      }
      if (resolvedPath && !isValidSessionPath(resolvedPath, engine.agentsDir)) {
        return c.json({ error: "Invalid session path" }, 403);
      }
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "session",
        studioId: requestContext.studioId,
        sessionPath: resolvedPath || engine.currentSessionPath || null,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      let messages;
      try {
        messages = await loadSessionHistoryMessages(engine, resolvedPath, {
          sessionId: querySessionId || engine.getSessionIdForPath?.(resolvedPath) || null,
        });
      } catch (err) {
        return c.json({ error: err.message }, 500);
      }
      const turns = foldMessagesIntoTurns(messages);
      return c.json({
        sessionId: querySessionId || engine.getSessionIdForPath?.(resolvedPath) || null,
        turns,
      });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  route.get("/conversation-map/layout", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.read", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      return c.json(await readLayoutFile(engine));
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  route.put("/conversation-map/layout", async (c) => {
    try {
      const requestContext = createRequestContext(c, engine);
      const auth = authorizeSessionRoute(requestContext, "sessions.write", {
        kind: "studio",
        studioId: requestContext.studioId,
      });
      if (!auth.allowed) return c.json({ error: "insufficient_scope", reason: auth.reason }, 403);
      const body = await c.req.json().catch(() => null);
      if (!body || typeof body !== "object" || Array.isArray(body)) {
        return c.json({ error: "invalid_body" }, 400);
      }
      const hasPositions = body.positions !== undefined;
      const hasCollapsed = body.collapsed !== undefined;
      if (!hasPositions && !hasCollapsed) {
        return c.json({ error: "invalid_body" }, 400);
      }
      // replacePositions=true 时整体替换坐标表（布局重置）；默认按 key 合并。
      const replacePositions = body.replacePositions === true;
      const file = layoutFilePath(engine);
      const current = await readLayoutFile(engine);
      const positions = hasPositions
        ? (replacePositions
          ? sanitizePositionsInput(body.positions)
          : { ...current.positions, ...sanitizePositionsInput(body.positions) })
        : current.positions;
      const collapsed = hasCollapsed ? sanitizeCollapsedInput(body.collapsed) : current.collapsed;
      const updatedAt = new Date().toISOString();
      const next = { version: 1, positions, collapsed, updatedAt };
      await fs.mkdir(path.dirname(file), { recursive: true });
      const tmpFile = `${file}.${process.pid}.tmp`;
      await fs.writeFile(tmpFile, JSON.stringify(next, null, 2), "utf-8");
      await fs.rename(tmpFile, file);
      return c.json({ ok: true, updatedAt });
    } catch (err) {
      return c.json({ error: err.message }, 500);
    }
  });

  return route;
}
