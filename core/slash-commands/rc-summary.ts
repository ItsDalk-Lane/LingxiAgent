/**
 * rc-summary.js — 为 /rc 接管生成桌面 session 简述（summarize slot fallback）
 *
 * 接管成功后要发一条带 summary 的回复给 bridge 用户，告诉 ta
 * "这个桌面会话之前聊了什么"。
 *
 * 新 Slot 架构下，summary 由语义 Slot "summarize" 提供：
 *   summarize slot 配置了模型 → 用该模型
 *   summarize slot 未配置 → fallback 到 chat 主聊天模型
 *   均失败 → null（上层自行兜底为 "已接管对话 <title>"）
 *
 * 不在此处做兜底文案；失败返回 null，调用方（/rc 选择 handler）决定最终文案，
 * 避免"摘要器"和"文案兜底"两个职责互相纠缠。
 */
import fs from "fs";
import { callText } from "../llm-client.ts";
import {
  callTextConfigFromResolvedModel,
} from "../model-execution-config.ts";
import { callTextWithLengthContract, type OutputLengthContract } from "../output-length-contract.ts";
import { getLocale } from "../../lib/i18n.ts";
import { isToolCallBlock } from "../llm-utils.ts";
import { createModuleLogger } from "../../lib/debug-log.ts";

const log = createModuleLogger("rc-summary");

const SUMMARY_TIMEOUT_MS = 15_000;
const CONTENT_CHAR_LIMIT = 1500;
const MAX_TURNS_FROM_TAIL = 8;

/**
 * @param {object} engine  engine.resolveAuxiliaryModelFresh()、engine.resolveModelWithCredentialsFresh(ref)
 * @param {object} agent   agent.config.models.chat 用于 chat fallback
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

  // summarize slot（未配置时 fallback 到 chat）
  const ctx = agent?.id ? { agentId: agent.id } : {};
  let summarizeResolved = null;
  try {
    summarizeResolved = await engine.resolveAuxiliaryModelFresh?.("summarize", ctx);
  } catch { /* ignore, fall through to chat */ }

  if (summarizeResolved?.model && summarizeResolved.baseUrl && summarizeResolved.api) {
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
    if (text) return text;
  }

  // chat model fallback（summarize slot 未配置或调用失败时）
  const chatRef = agent?.config?.models?.chat;
  if (chatRef?.id && chatRef?.provider) {
    try {
      const resolved = await engine.resolveModelWithCredentialsFresh?.({ id: chatRef.id, provider: chatRef.provider });
      if (resolved) {
        const text = await _safeCall({
          ...callTextConfigFromResolvedModel(resolved),
          usageLedger: engine.usageLedger,
          usageContext: usageContextForRc(engine, agent, sessionPath, "rc_summary_chat"),
          messages,
          lengthContract,
        }, "chat");
        if (text) return text;
      }
    } catch (err) {
      log.warn(`chat tier resolve failed: ${err.message}`);
    }
  }

  return null;
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
      : { kind: "utility", agentId: agent?.id ?? null },
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
