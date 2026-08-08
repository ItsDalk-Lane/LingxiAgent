/**
 * rc-summary.js — 为 /rc 接管生成桌面 session 简述（summarize slot）
 *
 * 接管成功后要发一条带 summary 的回复给 bridge 用户，告诉 ta
 * "这个桌面会话之前聊了什么"。
 *
 * 不变量：fallback 只发生一次，且完全收口在统一 resolver。
 * 本函数只表达「我要 summarize slot 的模型」，调用 resolver 一次，
 * 不再自行解析 chat、不再二次 fallback：
 *
 *   resolver 返回 resolved（summarize 显式配置，或未配置时 fallback 到 chat）
 *     → 调用该模型；运行时失败（timeout / 5xx / empty）→ 返回 null（best-effort）
 *   resolver 返回 null（无可用模型，例如未配置且 chat 缺失）
 *     → 返回 null
 *   resolver throw（summarize 显式配置错误：模型不存在 / 凭证缺失 / capability 不符）
 *     → 报告配置错误，返回 null；绝不回退到 chat
 *
 * 不在此处做兜底文案；失败返回 null，调用方（/rc 选择 handler）决定最终文案，
 * 避免"摘要器"和"文案兜底"两个职责互相纠缠。
 */
import fs from "fs";
import { callText } from "../llm-client.ts";
import { callTextWithLengthContract, type OutputLengthContract } from "../output-length-contract.ts";
import { getLocale } from "../../lib/i18n.ts";
import { isToolCallBlock } from "../llm-utils.ts";
import { isAuxiliaryConfigError } from "../auxiliary-model-resolver.ts";
import { createModuleLogger } from "../../lib/debug-log.ts";

const log = createModuleLogger("rc-summary");

const SUMMARY_TIMEOUT_MS = 15_000;
const CONTENT_CHAR_LIMIT = 1500;
const MAX_TURNS_FROM_TAIL = 8;

/**
 * @param {object} engine  engine.resolveAuxiliaryModelFresh("summarize", ctx)
 * @param {object} agent   agent.id 用于 resolver 上下文（cross-agent 正确 fallback）
 * @param {string} sessionPath  桌面 session 绝对路径
 * @returns {Promise<string|null>}
 */
export async function summarizeSessionForRc(engine, agent, sessionPath) {
  if (!sessionPath || !fs.existsSync(sessionPath)) return null;

  const content = _extractRecentTurns(sessionPath);
  if (!content.userText && !content.assistantText) return null;

  const isZh = getLocale().startsWith("zh");
  const messages = _buildMessages(content, isZh);
  const lengthContract = _summaryLengthContract(isZh);

  // 一次 resolve，一次 fallback——完全收口在 resolver。
  // resolver 对「未配置」按 Slot 策略 fallback（summarize→chat）；
  // 对「显式配置错误」直接 throw，这里不得吞掉后改用 chat。
  const ctx = agent?.id ? { agentId: agent.id } : {};
  let summarizeResolved = null;
  try {
    summarizeResolved = await engine.resolveAuxiliaryModelFresh?.("summarize", ctx);
  } catch (err) {
    // 显式配置错误：模型不存在 / 凭证缺失 / capability 不符。
    // 不得 fallback chat；返回 null 让上层走普通接管文案。
    if (isAuxiliaryConfigError(err)) {
      log.warn(`summarize slot 配置错误，摘要不可用（不回退 chat）: ${err.message}`);
    } else {
      log.warn(`summarize slot 解析异常: ${err.message}`);
    }
    return null;
  }

  if (!summarizeResolved?.model || !summarizeResolved.baseUrl || !summarizeResolved.api) {
    // 无可用模型（未配置且 chat 缺失），返回 null。
    return null;
  }

  const text = await _safeCall({
    api: summarizeResolved.api,
    apiKey: summarizeResolved.apiKey,
    baseUrl: summarizeResolved.baseUrl,
    headers: summarizeResolved.headers,
    model: summarizeResolved.model,
    usageLedger: summarizeResolved.usageLedger ?? engine.usageLedger,
    usageContext: usageContextForRc(engine, agent, sessionPath, "rc_summary_summarize"),
    messages,
    lengthContract,
  }, "summarize");
  return text;
}

function usageContextForRc(engine, agent, sessionPath, operation) {
  const sessionId = sessionPath ? engine?.getSessionIdForPath?.(sessionPath) || null : null;
  return {
    source: {
      subsystem: "phone",
      operation,
      surface: "bridge",
      trigger: "user",
    },
    attribution: sessionPath
      ? {
          kind: "session",
          ...(sessionId ? { sessionId } : {}),
          sessionPath,
          agentId: agent?.id ?? null,
        }
      : { kind: "auxiliary", agentId: agent?.id ?? null },
  };
}

function _summaryLengthContract(isZh): OutputLengthContract {
  return isZh
    ? { label: "/rc 摘要", target: 100, unit: "chars", min: 1, locale: "zh" }
    : { label: "/rc summary", target: 60, unit: "words", min: 1, locale: "en" };
}

async function _safeCall({ api, model, apiKey, baseUrl, headers, messages, usageLedger, usageContext, lengthContract }, tierLabel) {
  try {
    const { text } = await callTextWithLengthContract({
      callText,
      request: {
        api, model, apiKey, baseUrl, headers,
        signal: undefined,
        messages,
        temperature: 0.3,
        timeoutMs: SUMMARY_TIMEOUT_MS,
        usageLedger,
        usageContext,
      },
      contract: lengthContract,
    });
    return text?.trim() || null;
  } catch (err) {
    log.warn(`${tierLabel} tier failed: ${err.message}`);
    return null;
  }
}

/** 从 session jsonl 读最近几轮对话的 user/assistant text + tool names */
function _extractRecentTurns(sessionPath) {
  let raw;
  try { raw = fs.readFileSync(sessionPath, "utf-8"); }
  catch { return { userText: "", assistantText: "", tools: [] }; }

  const lines = raw.trim().split("\n").map(l => {
    try { return JSON.parse(l); } catch { return null; }
  }).filter(Boolean);

  const messages = lines
    .filter(l => l.type === "message" && l.message)
    .slice(-MAX_TURNS_FROM_TAIL);

  let userText = "";
  let assistantText = "";
  const tools = [];
  for (const line of messages) {
    const m = line.message;
    const textParts = (m.content || []).filter(c => c.type === "text").map(c => c.text).join("\n");
    if (m.role === "user" && textParts) {
      userText += (userText ? "\n---\n" : "") + textParts;
    }
    if (m.role === "assistant") {
      if (textParts) assistantText += (assistantText ? "\n---\n" : "") + textParts;
      const toolParts = (m.content || []).filter(isToolCallBlock);
      for (const tp of toolParts) tools.push(tp.name || "unknown_tool");
    }
  }
  return {
    userText: userText.slice(0, CONTENT_CHAR_LIMIT),
    assistantText: assistantText.slice(0, CONTENT_CHAR_LIMIT),
    tools: [...new Set(tools)],
  };
}

function _buildMessages({ userText, assistantText, tools }, isZh) {
  const system = isZh
    ? `你是对话摘要生成器。根据下面几轮对话，概括这个桌面会话正在处理什么、当前进展，以及能看出的下一步线索。
规则：中文，直接输出 1-3 句，目标约 100 字，可在 60-200 字之间自然浮动；不加引号、不加前缀、不列编号；不要逐条复述工具日志，也不要只写工具名或泛泛一句。`
    : `You summarize conversations. Given the turns below, describe what this desktop session is handling, its current progress, and any visible next-step clue.
Rules: output 1-3 direct English sentences, aiming for about 60 words; 36-120 words is acceptable. No quotes, preamble, or numbering; do not list tool logs, and do not reduce the summary to tool names or a generic phrase.`;

  const toolStr = tools.length > 0
    ? (isZh ? `\n用到的工具：${tools.join("、")}` : `\nTools used: ${tools.join(", ")}`)
    : "";

  const contextLabel = isZh ? "对话片段" : "Conversation";
  return [
    { role: "system", content: system },
    {
      role: "user",
      content: `${contextLabel}：\n\n${isZh ? "用户：" : "User: "}${userText}\n\n${isZh ? "助手：" : "Assistant: "}${assistantText}${toolStr}`,
    },
  ];
}
