// 跨 session 投递单点：一切以 sessionId 为键，path 仅在 manifest 解析后的最后一步出现。
// 空闲目标走完整回合提交；跑动中目标走 interjection（与用户手动"跑动中插入"同款语义）。
import {
  submitDesktopSessionMessageWithReceipt,
  submitDesktopSessionInterjection,
  isDesktopInputRejectedBeforeAcceptance,
} from "../../core/desktop-session-submit.ts";
import { t } from "../i18n.ts";

export const AGENT_MESSAGE_SOURCE = "agent_session";

export function buildAgentMessagePrefix(agentName: string): string {
  return t("sessionCollab.messagePrefix", { name: agentName || "Agent" });
}

export async function deliverAgentMessage(engine: any, opts: {
  targetSessionId: string;
  message: string;
  from: { agentId: string | null; agentName: string | null };
}): Promise<{ accepted: true; targetSessionId: string }> {
  const targetSessionId = String(opts.targetSessionId || "").trim();
  const manifest = engine.getSessionManifest?.(targetSessionId) || null;
  const sessionPath = manifest?.currentLocator?.path || null;
  if (!sessionPath) throw new Error(`session_not_found:${targetSessionId}`);

  const prefix = buildAgentMessagePrefix(opts.from.agentName || opts.from.agentId || "Agent");
  const text = `${prefix}\n${opts.message}`;
  const displayMessage = {
    text: opts.message,
    source: AGENT_MESSAGE_SOURCE,
    origin: { kind: "agent", agentId: opts.from.agentId, agentName: opts.from.agentName },
  };
  const payload = { sessionId: targetSessionId, sessionPath, text, displayMessage };

  const submitAccepted = () => {
    const submission = submitDesktopSessionMessageWithReceipt(engine, payload);
    submission.completion.catch((err: any) => {
      console.warn("[session-collab] message submission failed:", err?.message || err);
    });
    return submission.accepted;
  };

  const streaming = engine.isSessionStreaming?.(sessionPath) === true;
  const primary = streaming
    ? () => submitDesktopSessionInterjection(engine, payload)
    : submitAccepted;
  const fallback = streaming
    ? submitAccepted
    : () => submitDesktopSessionInterjection(engine, payload);

  try {
    await primary();
  } catch (err: any) {
    // 只对已经证明未接受的忙态竞态换路一次，不能按错误文案重发结果未知的输入。
    if (err?.message !== "session_busy" || !isDesktopInputRejectedBeforeAcceptance(err)) throw err;
    await fallback();
  }
  return { accepted: true, targetSessionId };
}
