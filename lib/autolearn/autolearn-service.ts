/**
 * autolearn-service — 踩坑自动沉淀的提炼与建议链（阶段一·6）。
 *
 * 触发：server/autolearn-handler 在 turn 结束缝把本轮工具轨迹交过来。
 * 闸门顺序（任一不过即静默结束，只有走到建议卡才让用户感知）：
 *   aborted → 计划模式（read_only）→ 总开关 → 工具调用数阈值 → 频率防抖
 *   → 辅助模型提炼（summarize 槽，未配置则跳过，不回退主模型）
 *   → 名称消毒/同名冲突/大小上限 → guard 槽安全审查（fail → 丢弃只记日志）
 *   → ConfirmStore 建议卡（确认才落盘，超时/拒绝即弃，绝不静默写技能）。
 *
 * 频率防抖为进程内存态（每会话 30 分钟 + 全局每日 10 次），重启清零——
 * 这是有意的保守：宁可重启后多提炼一次，也不把防抖状态落盘成新的持久化面。
 *
 * 诚实声明：建议卡是实时的——刷新/重开会话后卡片不再出现（不落盘），
 * ConfirmStore 条目超时后确认接口返回 404，前端据此刻画「已过期」。
 */

import fs from "node:fs";
import path from "node:path";
import { t } from "../i18n.ts";
import { MAX_SKILL_SIZE, safetyReview } from "../tools/install-skill.ts";
import { sanitizeSkillName } from "../skills/skill-package-installer.ts";
import {
  assembleLessonSkillContent,
  installLearnedLesson,
} from "../tools/learn-lesson-tool.ts";
import { callText } from "../../core/llm-client.ts";
import { callTextConfigFromResolvedModel } from "../../core/model-execution-config.ts";
import { isReadOnlyPermissionMode } from "../../core/session-permission-mode.ts";

/** 本轮工具调用数达到阈值才考虑提炼（低于此数的短轮次通常没有可复用教训）。 */
export const AUTOLEARN_MIN_TOOL_CALLS = 8;
/** 同一会话两次提炼建议的最小间隔。 */
export const AUTOLEARN_SESSION_COOLDOWN_MS = 30 * 60_000;
/** 全局每日建议上限（所有会话合计）。 */
export const AUTOLEARN_DAILY_CAP = 10;
/** 建议卡等待用户决策的时长（ConfirmStore 总是带超时，这里放长到一天）。 */
export const AUTOLEARN_CONFIRM_TIMEOUT_MS = 24 * 60 * 60_000;
/** 喂给提炼模型的轨迹条目上限与单条截断（防御性，handler 侧已先截一道）。 */
export const AUTOLEARN_MAX_TRACE_ENTRIES = 40;
export const AUTOLEARN_TRACE_ENTRY_CHARS = 200;
/** 提炼产物的字段上限（name 走 sanitizeSkillName 的 32 字符约定，此处卡输入侧）。 */
export const AUTOLEARN_MAX_DESCRIPTION_CHARS = 300;
export const AUTOLEARN_MAX_LESSON_CHARS = 4000;

const DISTILL_TIMEOUT_MS = 60_000;

export interface AutolearnTraceEntry {
  name: string;
  ok: boolean;
  head?: string;
}

export interface AutolearnTurnSummary {
  sessionPath: string;
  toolCalls: number;
  trace: AutolearnTraceEntry[];
  aborted: boolean;
  permissionMode: string | null;
}

export interface AutolearnSuggestion {
  confirmId: string;
  name: string;
  description: string;
  lesson: string;
}

export interface AutolearnDeps {
  /** 全局开关（preferences.autolearn.enabled，默认开）。 */
  isEnabled: () => boolean;
  /** engine.resolveAuxiliaryModelFresh 的绑定；slot=distill 用 summarize，审查用 guard。 */
  resolveAuxModel: (slot: string, options: { sessionPath?: string }) => Promise<any>;
  /** 用户技能池目录（learn_lesson 同一落点）。 */
  getUserSkillsDir: () => string | null;
  /** 由 sessionPath 反查 agent 目录（经验库索引落点）。 */
  getAgentDirForSession: (sessionPath: string) => string | null;
  /** ConfirmStore（或测试替身）：create(kind, payload, sessionRef, timeoutMs) → {confirmId, promise}。 */
  confirmStore: {
    create: (kind: string, payload: any, sessionRef: string, timeoutMs: number) => { confirmId: string; promise: Promise<any> };
  };
  /** 事件出口（server 侧绑定到 hub.eventBus.emit）。 */
  emitEvent: (event: any, sessionPath: string) => void;
  /** 落盘成功后刷新技能池（agent-manager 的 install 回调链）；带 sessionPath 供反查归属 agent。 */
  notifyInstalled?: (skillName: string, sessionPath: string) => void;
  log?: { info?: (...args: any[]) => void; warn?: (...args: any[]) => void };
  now?: () => number;
}

/** 提炼提示词：严格 JSON 双形状——有教训给三字段，没有给 {"skip":true}。 */
export function buildAutolearnDistillPrompt(trace: AutolearnTraceEntry[]): string {
  const lines = trace.slice(0, AUTOLEARN_MAX_TRACE_ENTRIES).map((entry, i) => {
    const head = typeof entry.head === "string" && entry.head
      ? ` — ${entry.head.slice(0, AUTOLEARN_TRACE_ENTRY_CHARS)}`
      : "";
    return `${i + 1}. ${entry.name} [${entry.ok ? "ok" : "error"}]${head}`;
  });
  return [
    "You are reviewing one agent turn's tool-call trace to decide whether it contains a reusable lesson worth crystallizing into a skill.",
    "A lesson is worth keeping only if it is: reusable across sessions, non-obvious (a pitfall hit, a recovery technique, a project-specific gotcha), and concrete.",
    "Do NOT crystallize: ordinary successful flows, one-off facts, anything already obvious from the tool names, or secrets/credentials/paths with sensitive values.",
    "",
    "Reply with STRICT JSON only, one of:",
    '{"name":"kebab-case-skill-name","description":"one sentence","lesson":"markdown body of the lesson"}',
    '{"skip":true}',
    "",
    "Tool-call trace of the turn:",
    ...lines,
  ].join("\n");
}

/** 解析提炼结果；任何偏差（非 JSON、缺字段、超限）都归一为 null，不猜。 */
export function parseAutolearnDistillResult(raw: string | null | undefined): { name: string; description: string; lesson: string } | null {
  if (typeof raw !== "string" || !raw.trim()) return null;
  let text = raw.trim();
  // 模型常包一层 ```json 围栏，剥掉再解析；剥完仍不是 JSON 就放弃。
  const fenced = text.match(/^```(?:json)?\s*\n([\s\S]*?)\n```$/);
  if (fenced) text = fenced[1].trim();
  let parsed: any;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
  if (parsed.skip === true) return null;
  const name = typeof parsed.name === "string" ? parsed.name.trim() : "";
  const description = typeof parsed.description === "string" ? parsed.description.trim() : "";
  const lesson = typeof parsed.lesson === "string" ? parsed.lesson.trim() : "";
  if (!name || !description || !lesson) return null;
  if (description.length > AUTOLEARN_MAX_DESCRIPTION_CHARS || lesson.length > AUTOLEARN_MAX_LESSON_CHARS) return null;
  return { name, description, lesson };
}

export function createAutolearnService(deps: AutolearnDeps) {
  const now = deps.now || (() => Date.now());
  const log = deps.log || {};
  /** 每会话上次建议时间 + 全局当日计数（内存态，重启清零，见文件头说明）。 */
  const lastSuggestBySession = new Map<string, number>();
  let dailyCount = 0;
  let dailyDay = "";

  function dayKey(ts: number): string {
    return new Date(ts).toISOString().slice(0, 10);
  }

  function debounceBlocked(sessionPath: string, ts: number): boolean {
    const day = dayKey(ts);
    if (day !== dailyDay) {
      dailyDay = day;
      dailyCount = 0;
    }
    if (dailyCount >= AUTOLEARN_DAILY_CAP) return true;
    const last = lastSuggestBySession.get(sessionPath);
    return typeof last === "number" && ts - last < AUTOLEARN_SESSION_COOLDOWN_MS;
  }

  function markSuggested(sessionPath: string, ts: number): void {
    lastSuggestBySession.set(sessionPath, ts);
    dailyCount += 1;
  }

  async function distill(trace: AutolearnTraceEntry[], sessionPath: string): Promise<{ name: string; description: string; lesson: string } | null> {
    let resolved: any = null;
    try {
      resolved = await deps.resolveAuxModel("summarize", { sessionPath });
    } catch (err) {
      log.warn?.(`[autolearn] distill model resolve failed: ${(err as any)?.message || err}`);
      return null;
    }
    // summarize 槽未配置 → 跳过（显式降级：不回退主模型，不打扰用户）。
    if (!resolved) return null;
    try {
      const result = await callText({
        ...callTextConfigFromResolvedModel(resolved),
        messages: [{ role: "user", content: buildAutolearnDistillPrompt(trace) }],
        temperature: 0.2,
        maxTokens: 1200,
        timeoutMs: DISTILL_TIMEOUT_MS,
        usageLedger: resolved.usageLedger,
        usageContext: {
          source: { subsystem: "autolearn", operation: "distill", surface: "system", trigger: "turn_end" },
          attribution: {
            kind: "autolearn",
            agentId: resolved.usageAgentId || null,
            sessionPath: resolved.usageSessionPath || sessionPath,
            sessionId: resolved.usageSessionId || null,
          },
        },
      });
      const text = typeof result === "string" ? result : result?.text;
      return parseAutolearnDistillResult(text);
    } catch (err) {
      log.warn?.(`[autolearn] distill call failed: ${(err as any)?.message || err}`);
      return null;
    }
  }

  async function settleSuggestion(confirmId: string, promise: Promise<any>, prepared: {
    sessionPath: string;
    safeName: string;
    description: string;
    lesson: string;
    content: string;
  }): Promise<void> {
    let outcome: any;
    try {
      outcome = await promise;
    } catch {
      return; // ConfirmStore 自身异常（如 store 关闭）——建议作废，不升级。
    }
    if (!outcome || outcome.action !== "confirmed") return; // timeout/rejected/aborted 皆静默丢弃

    const userSkillsDir = deps.getUserSkillsDir();
    const agentDir = deps.getAgentDirForSession(prepared.sessionPath);
    if (!userSkillsDir || !agentDir) return;
    // 确认瞬间再查一次冲突：建议存活期间用户可能已装同名技能。
    if (fs.existsSync(path.join(userSkillsDir, prepared.safeName))) return;
    try {
      const installed = await installLearnedLesson({
        agentDir,
        userSkillsDir,
        safeName: prepared.safeName,
        content: prepared.content,
        description: prepared.description,
        lesson: prepared.lesson,
      });
      deps.notifyInstalled?.(installed.skillName, prepared.sessionPath);
      deps.emitEvent({
        type: "notification",
        title: t("autolearn.installed.title"),
        body: t("autolearn.installed.body", { name: installed.skillName }),
      }, prepared.sessionPath);
    } catch (err) {
      // 用户已确认但落盘失败——红线「禁止静默降级」：发通知如实告知。
      log.warn?.(`[autolearn] install after confirm failed: ${(err as any)?.message || err}`);
      deps.emitEvent({
        type: "notification",
        title: t("autolearn.installFailed.title"),
        body: t("autolearn.installFailed.body", { name: prepared.safeName }),
      }, prepared.sessionPath);
    }
  }

  return {
    /** 主入口：turn 结束缝调用。全部闸门走完后要么发出建议卡，要么无声结束。 */
    async observeTurn(summary: AutolearnTurnSummary): Promise<void> {
      const sessionPath = summary?.sessionPath;
      if (!sessionPath || typeof sessionPath !== "string") return;
      if (summary.aborted) return;
      if (isReadOnlyPermissionMode(summary.permissionMode)) return; // 计划模式只读档不沉淀
      if (!deps.isEnabled()) return;
      if (!Number.isFinite(summary.toolCalls) || summary.toolCalls < AUTOLEARN_MIN_TOOL_CALLS) return;
      const ts = now();
      if (debounceBlocked(sessionPath, ts)) return;

      const distilled = await distill(summary.trace || [], sessionPath);
      if (!distilled) return;

      const safeName = sanitizeSkillName(distilled.name);
      if (!safeName) return;
      const userSkillsDir = deps.getUserSkillsDir();
      const agentDir = deps.getAgentDirForSession(sessionPath);
      if (!userSkillsDir || !agentDir) return;
      if (fs.existsSync(path.join(userSkillsDir, safeName))) return; // 同名冲突：跳过，不覆盖

      const content = assembleLessonSkillContent(safeName, distilled.description, distilled.lesson);
      if (content.length > MAX_SKILL_SIZE) return;

      // 与 install_skill/learn_lesson 同一条 guard 审查；失败即丢弃建议（只记日志，
      // 绝不在未经审查时把内容推到用户面前）。
      const review = await safetyReview(content, () => deps.resolveAuxModel("guard", { sessionPath }));
      if (!review.safe) {
        log.info?.(`[autolearn] lesson "${safeName}" dropped by safety review: ${review.reason || "no reason"}`);
        return;
      }

      // 到这一步才占用防抖配额：提炼与审查失败不消耗用户的每日额度。
      markSuggested(sessionPath, ts);
      const { confirmId, promise } = deps.confirmStore.create(
        "autolearn_lesson",
        { name: safeName, description: distilled.description, lesson: distilled.lesson },
        sessionPath,
        AUTOLEARN_CONFIRM_TIMEOUT_MS,
      );
      deps.emitEvent({
        type: "autolearn_suggestion",
        confirmId,
        name: safeName,
        description: distilled.description,
        lesson: distilled.lesson,
      }, sessionPath);
      // 结算链挂后台：确认才落盘，其余结局无声。
      void settleSuggestion(confirmId, promise, {
        sessionPath,
        safeName,
        description: distilled.description,
        lesson: distilled.lesson,
        content,
      });
    },
    /** 测试探针：读防抖内部状态（不写）。 */
    _debugState() {
      return { lastSuggestBySession: new Map(lastSuggestBySession), dailyCount, dailyDay };
    },
  };
}
