/**
 * ask-user-tool.ts — ask_user 工具
 *
 * 结构化提问：agent 在真正的决策点上向用户批量提问（单选/多选/文本），
 * 用户在输入区的提问卡上作答，答案作为工具结果返回。
 *
 * 设计要点：
 *   - 复用 ConfirmStore 阻塞确认链 + 输入区 session_confirmation 卡
 *     （与 mcp_elicitation / session_folders 同一条道）；
 *   - 权限为 read 级：任何权限模式（含计划模式）都放行——计划模式的
 *     「收工前交决策」正依赖本工具；
 *   - 选项 label 在同题内必须唯一（防注入歧义：两个同名选项用户无从分辨）；
 *   - 超时自动选推荐项并如实回传 autoSelected 标记；无推荐项的题目
 *     超时后如实标注 unanswered，不编造答案；
 *   - 用户「暂不回答」是合法结果（dismissed），不是工具错误。
 */

import { StringEnum, Type } from "../pi-sdk/index.ts";
import { t } from "../i18n.ts";
import { getToolSessionPath } from "./tool-session.ts";
import { toolError, toolOk } from "./tool-result.ts";

/** 一次最多问 8 题：再多说明该拆轮次，而不是糊用户一堵墙 */
const MAX_QUESTIONS = 8;
/** 单题选项数上下限：少于 2 个不构成选择，多于 12 个用户读不动 */
const MIN_OPTIONS = 2;
const MAX_OPTIONS = 12;
const MAX_QUESTION_CHARS = 2000;
const MAX_OPTION_VALUE_CHARS = 200;
const MAX_OPTION_LABEL_CHARS = 200;
const MAX_OPTION_DESC_CHARS = 500;
const MAX_TEXT_ANSWER_CHARS = 4000;
const KEY_PATTERN = /^[a-zA-Z0-9_-]{1,32}$/;
const DEFAULT_TIMEOUT_MS = 5 * 60 * 1000;
const MIN_TIMEOUT_SECONDS = 30;
const MAX_TIMEOUT_SECONDS = 30 * 60;

type AskOption = { value: string; label: string; description?: string };
type AskQuestion = {
  key: string;
  question: string;
  type: "single" | "multi" | "text";
  options: AskOption[] | null;
  recommended: string[];
  required: boolean;
};

function invalid(reason: string) {
  return toolError(t("error.askUserInvalidQuestions", { reason }), { reason });
}

/**
 * 校验并规范化 questions。返回 { questions } 或 { error }（ToolResult）。
 * 所有拒绝都发生在烧确认卡之前。
 */
function normalizeQuestions(raw: unknown): { questions: AskQuestion[] } | { error: any } {
  if (!Array.isArray(raw) || raw.length === 0 || raw.length > MAX_QUESTIONS) {
    return { error: invalid(`questions must be an array of 1..${MAX_QUESTIONS} items`) };
  }
  const usedKeys = new Set<string>();
  const questions: AskQuestion[] = [];
  for (const [index, entry] of raw.entries()) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      return { error: invalid(`question #${index + 1} must be an object`) };
    }
    const q = entry as Record<string, unknown>;
    const question = typeof q.question === "string" ? q.question.trim() : "";
    if (!question || question.length > MAX_QUESTION_CHARS) {
      return { error: invalid(`question #${index + 1} needs a non-empty question (<= ${MAX_QUESTION_CHARS} chars)`) };
    }
    const key = typeof q.key === "string" && q.key.trim() ? q.key.trim() : `q${index + 1}`;
    if (!KEY_PATTERN.test(key)) {
      return { error: invalid(`question key "${key}" must match ${KEY_PATTERN}`) };
    }
    if (usedKeys.has(key)) {
      return { error: invalid(`duplicate question key "${key}"`) };
    }
    usedKeys.add(key);

    const hasOptions = Array.isArray(q.options);
    const type = typeof q.type === "string" && q.type
      ? q.type
      : (hasOptions ? "single" : "text");
    if (type !== "single" && type !== "multi" && type !== "text") {
      return { error: invalid(`question "${key}" has unknown type "${type}"`) };
    }
    const required = q.required !== false;

    if (type === "text") {
      if (hasOptions || q.recommended !== undefined) {
        return { error: invalid(`text question "${key}" cannot carry options or recommended`) };
      }
      questions.push({ key, question, type, options: null, recommended: [], required });
      continue;
    }

    if (!hasOptions || (q.options as unknown[]).length < MIN_OPTIONS || (q.options as unknown[]).length > MAX_OPTIONS) {
      return { error: invalid(`question "${key}" needs ${MIN_OPTIONS}..${MAX_OPTIONS} options`) };
    }
    const seenValues = new Set<string>();
    const seenLabels = new Set<string>();
    const options: AskOption[] = [];
    for (const rawOption of q.options as unknown[]) {
      if (!rawOption || typeof rawOption !== "object" || Array.isArray(rawOption)) {
        return { error: invalid(`question "${key}" has a non-object option`) };
      }
      const o = rawOption as Record<string, unknown>;
      const value = typeof o.value === "string" ? o.value.trim() : "";
      if (!value || value.length > MAX_OPTION_VALUE_CHARS) {
        return { error: invalid(`question "${key}" has an option with empty/oversized value`) };
      }
      if (seenValues.has(value)) {
        return { error: invalid(`question "${key}" repeats option value "${value}"`) };
      }
      seenValues.add(value);
      const label = typeof o.label === "string" && o.label.trim() ? o.label.trim() : value;
      if (label.length > MAX_OPTION_LABEL_CHARS) {
        return { error: invalid(`question "${key}" has an option label over ${MAX_OPTION_LABEL_CHARS} chars`) };
      }
      if (seenLabels.has(label)) {
        return { error: invalid(`question "${key}" repeats option label "${label}"`) };
      }
      seenLabels.add(label);
      const description = typeof o.description === "string" && o.description.trim()
        ? o.description.trim().slice(0, MAX_OPTION_DESC_CHARS)
        : undefined;
      options.push(description ? { value, label, description } : { value, label });
    }

    const rawRecommended = q.recommended;
    const recommendedList = rawRecommended === undefined || rawRecommended === null
      ? []
      : Array.isArray(rawRecommended) ? rawRecommended : [rawRecommended];
    const recommended: string[] = [];
    for (const item of recommendedList) {
      if (typeof item !== "string" || !seenValues.has(item)) {
        return { error: invalid(`question "${key}" recommends an unknown option value`) };
      }
      if (!recommended.includes(item)) recommended.push(item);
    }
    if (type === "single" && recommended.length > 1) {
      return { error: invalid(`single-choice question "${key}" can recommend at most one option`) };
    }
    questions.push({ key, question, type, options, recommended, required });
  }
  return { questions };
}

/** 把前端回传的原始 value 按题目定义映射回 { values, labels }，未知值丢弃。 */
function mapAnswer(q: AskQuestion, raw: unknown): { values: string[]; labels: string[] } {
  if (q.type === "text") {
    const text = typeof raw === "string" ? raw.trim().slice(0, MAX_TEXT_ANSWER_CHARS) : "";
    return text ? { values: [text], labels: [text] } : { values: [], labels: [] };
  }
  const lookup = new Map((q.options || []).map((o) => [o.value, o.label]));
  const picked = q.type === "multi"
    ? (Array.isArray(raw) ? raw : [])
    : (typeof raw === "string" ? [raw] : []);
  const values: string[] = [];
  const labels: string[] = [];
  for (const item of picked) {
    if (typeof item !== "string" || !lookup.has(item) || values.includes(item)) continue;
    values.push(item);
    labels.push(lookup.get(item) as string);
    if (q.type === "single") break;
  }
  return { values, labels };
}

function recommendedAnswer(q: AskQuestion): { values: string[]; labels: string[] } {
  if (!q.recommended.length) return { values: [], labels: [] };
  const lookup = new Map((q.options || []).map((o) => [o.value, o.label]));
  return {
    values: [...q.recommended],
    labels: q.recommended.map((v) => lookup.get(v) || v),
  };
}

function formatAnswerLines(answers: any[]): string {
  return answers.map((a) => {
    const picked = a.labels.length ? a.labels.join(", ") : t("approval.askUser.unanswered");
    const auto = a.autoSelected ? ` ${t("approval.askUser.autoMarker")}` : "";
    return `- ${a.question} → ${picked}${auto}`;
  }).join("\n");
}

/**
 * @param {object} deps
 * @param {() => object|null} deps.getConfirmStore  阻塞确认存储
 * @param {() => string|null} deps.getSessionPath   焦点会话兜底（优先 ctx）
 * @param {(event: object, sessionPath: string) => void} deps.emitEvent  引擎事件出口
 */
export function createAskUserTool(deps: Record<string, any> = {}) {
  return {
    name: "ask_user",
    label: "Ask User",
    description:
      "Ask the user one or more structured questions (single-choice, multi-choice, or free text) in a batched card, and wait for the answers. Use at genuine decision points where the user's intent changes what you do next — never for things you could reasonably decide or look up yourself. Prefer options over free text, and always mark a 'recommended' option when you have a defensible default: if the card times out, recommended answers are auto-selected and the result says so. Batch related questions into one call instead of asking back-to-back. The user may dismiss the card; that is a valid outcome, not an error.",
    sessionPermission: {
      // 提问本身不改变任何系统状态——任何权限模式（含只读/计划模式）都放行
      resolveInvocation: (_params: any = {}) => ({
        action: "ask",
        kind: "read",
        capability: "ask_user.ask",
      }),
    },
    parameters: Type.Object({
      questions: Type.Array(
        Type.Object({
          key: Type.Optional(Type.String({ description: "Answer map key (letters/digits/_/-, <=32). Defaults to q1, q2, ..." })),
          question: Type.String({ description: "The question text shown to the user." }),
          type: Type.Optional(StringEnum(["single", "multi", "text"] as const, {
            description: "Defaults to 'single' when options are given, 'text' otherwise.",
          })),
          options: Type.Optional(Type.Array(Type.Object({
            value: Type.String({ description: "Stable machine value returned in the answer." }),
            label: Type.Optional(Type.String({ description: "Display text; must be unique within the question. Defaults to value." })),
            description: Type.Optional(Type.String({ description: "One line explaining this option's trade-off." })),
          }))),
          recommended: Type.Optional(Type.Union([Type.String(), Type.Array(Type.String())], {
            description: "Option value(s) auto-selected on timeout. Single-choice takes one value.",
          })),
          required: Type.Optional(Type.Boolean({ description: "Default true; optional questions may be left unanswered." })),
        }),
        { description: "1..8 questions asked together in one card." },
      ),
      timeout_seconds: Type.Optional(Type.Number({
        description: `Seconds to wait before auto-selecting recommended answers (${MIN_TIMEOUT_SECONDS}..${MAX_TIMEOUT_SECONDS}, default 300).`,
      })),
    }),
    execute: async (_toolCallId: string, params: any, _signal: any, _onUpdate: any, ctx: any) => {
      const normalized = normalizeQuestions(params?.questions);
      if ("error" in normalized) return normalized.error;
      const questions = normalized.questions;

      const confirmStore = deps.getConfirmStore?.() || deps.confirmStore || null;
      const sessionPath = getToolSessionPath(ctx) || deps.getSessionPath?.() || null;
      if (!confirmStore || !sessionPath) {
        return toolError(t("error.askUserUnavailable"));
      }

      const rawTimeout = Number(params?.timeout_seconds);
      const timeoutMs = Number.isFinite(rawTimeout) && rawTimeout > 0
        ? Math.min(MAX_TIMEOUT_SECONDS, Math.max(MIN_TIMEOUT_SECONDS, Math.round(rawTimeout))) * 1000
        : DEFAULT_TIMEOUT_MS;

      const { confirmId, promise } = confirmStore.create(
        "ask_user",
        { questions },
        sessionPath,
        timeoutMs,
      );
      deps.emitEvent?.({
        type: "session_confirmation",
        request: {
          type: "session_confirmation",
          confirmId,
          kind: "ask_user",
          surface: "input",
          status: "pending",
          title: t("approval.askUser.title"),
          body: questions.length === 1
            ? questions[0].question
            : t("approval.askUser.body", { count: questions.length }),
          subject: { label: t("approval.askUser.subject"), detail: "" },
          severity: "normal",
          actions: {
            confirmLabel: t("approval.askUser.submit"),
            rejectLabel: t("approval.askUser.dismiss"),
          },
          payload: { questions },
        },
      }, sessionPath);

      const decision = await promise;
      const action = decision?.action;
      const value = decision?.value;

      if (action === "confirmed") {
        const answers = questions.map((q) => {
          const mapped = mapAnswer(q, value?.[q.key]);
          return {
            key: q.key,
            question: q.question,
            values: mapped.values,
            labels: mapped.labels,
            unanswered: mapped.values.length === 0,
            autoSelected: false,
          };
        });
        const message = `${t("approval.askUser.answered")}\n${formatAnswerLines(answers)}`;
        return toolOk(message, { answered: true, timedOut: false, answers });
      }

      if (action === "timeout") {
        const answers = questions.map((q) => {
          const auto = recommendedAnswer(q);
          return {
            key: q.key,
            question: q.question,
            values: auto.values,
            labels: auto.labels,
            unanswered: auto.values.length === 0,
            autoSelected: auto.values.length > 0,
          };
        });
        const message = `${t("approval.askUser.timedOut")}\n${formatAnswerLines(answers)}`;
        return toolOk(message, { answered: true, timedOut: true, answers });
      }

      if (action === "aborted") {
        return toolOk(t("approval.askUser.aborted"), { answered: false, aborted: true, answers: [] });
      }
      return toolOk(t("approval.askUser.dismissed"), { answered: false, dismissed: true, answers: [] });
    },
  };
}
