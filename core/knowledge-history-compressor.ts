/**
 * knowledge-history-compressor —— 历史轮知识注入块的编号压缩（模型侧重发前）。
 *
 * [KnowledgeContext] 注入块随 user message 持久化；不压缩时 SDK 每轮逐字重发，
 * 旧轮知识全文一直占用上下文。本模块在 agent 消息重建点（提交新消息 / 分支
 * 重建）把历史轮的注入块替换为"编号清单"：块头（含 sourceId + ordinal）保留、
 * 正文省略，模型需要原文时可用 knowledge_read 工具按编号回查。
 *
 * 边界（与 stripSessionReminderBlocks 同一语义）：
 * - 只处理 role==="user" 的消息（注入块只拼进 user message）；
 * - 未闭合信封 fail-closed 替换到文本尾（残缺 JSONL 防泄漏正文）；
 * - 已压缩块（无证据块头行）再跑一次是 no-op（幂等）；
 * - JSONL 存储永不改写——压缩只发生在发给模型的内存消息列表上，
 *   preservePromptEnvelope 的逐字重放路径不经过本模块。
 */
import { KNOWLEDGE_CONTEXT_BLOCK_END, KNOWLEDGE_CONTEXT_BLOCK_PREFIX } from "./session-reminders.ts";

/** 块头行：[K1] notebook "..." / source "..." (sourceId: src-xxx) / chunk ordinal 5 */
const EVIDENCE_HEADER_RE = /^\[K(\d+)\] notebook "[^"]*" \/ source "[^"]*" \(sourceId: ([^)]+)\) \/ chunk ordinal (\d+)/;

interface CompressedKnowledgeBlock {
  headerLines: string[];
}

/** 解析信封内的证据块头行；无 sourceId 的历史旧块头不匹配（退化为计数）。 */
function parseEvidenceHeaders(blockBody: string): CompressedKnowledgeBlock {
  const headerLines: string[] = [];
  for (const line of blockBody.split(/\r?\n/)) {
    const match = line.match(EVIDENCE_HEADER_RE);
    if (match) {
      headerLines.push(`- ${match[0]}`);
    }
  }
  return { headerLines };
}

/** 单条 user message 文本的压缩替换。返回 null 表示无需改动。 */
function compressMessageText(text: string): string | null {
  if (!text.includes(KNOWLEDGE_CONTEXT_BLOCK_PREFIX)) return null;
  const lines = text.split(/\r?\n/);
  const out: string[] = [];
  let inside = false;
  let blockBody: string[] = [];
  let changed = false;
  const flushBlock = () => {
    const body = blockBody.join("\n");
    const { headerLines } = parseEvidenceHeaders(body);
    if (headerLines.length === 0) {
      // 已压缩块（幂等重入）或旧格式无 sourceId：原样保留。
      out.push(KNOWLEDGE_CONTEXT_BLOCK_PREFIX, body, KNOWLEDGE_CONTEXT_BLOCK_END);
      return;
    }
    changed = true;
    out.push(
      KNOWLEDGE_CONTEXT_BLOCK_PREFIX,
      "Knowledge evidence retrieved in an earlier turn of this conversation; full content omitted to save context.",
      `Evidence blocks retrieved in that turn: ${headerLines.length}.`,
      ...headerLines,
      "The blocks above are addressable: use the `knowledge_read` tool with the current turn's scopeId (from the latest [KnowledgeContext] block's Scope line), a sourceId and fromOrdinal/toOrdinal to re-read any of them if the answer needs the original text.",
      KNOWLEDGE_CONTEXT_BLOCK_END,
    );
  };
  for (const line of lines) {
    if (!inside && line.trim() === KNOWLEDGE_CONTEXT_BLOCK_PREFIX) {
      inside = true;
      blockBody = [];
      continue;
    }
    if (inside && line.trim() === KNOWLEDGE_CONTEXT_BLOCK_END) {
      inside = false;
      flushBlock();
      continue;
    }
    if (inside) {
      blockBody.push(line);
      continue;
    }
    out.push(line);
  }
  if (inside) {
    // 未闭合信封 fail-closed：按已收集体替换（防残缺正文泄漏给模型）。
    flushBlock();
    changed = true;
  }
  return changed ? out.join("\n") : null;
}

/**
 * 把消息列表中所有 user 消息的历史知识注入块替换为编号清单。
 * 不可变：返回新数组（无改动时原样返回引用并标记 changed=false）。
 */
export function compressHistoricalKnowledgeContextMessages(
  messages: unknown,
): { messages: unknown; changed: boolean } {
  if (!Array.isArray(messages)) return { messages, changed: false };
  let changed = false;
  const next = messages.map((message) => {
    if (!message || typeof message !== "object" || (message as any).role !== "user") return message;
    const content = (message as any).content;
    if (typeof content === "string") {
      const compressed = compressMessageText(content);
      if (compressed == null) return message;
      changed = true;
      return { ...message, content: compressed };
    }
    if (Array.isArray(content)) {
      let blockChanged = false;
      const nextContent = content.map((block) => {
        if (!block || typeof block !== "object" || typeof (block as any).text !== "string") return block;
        const compressed = compressMessageText((block as any).text);
        if (compressed == null) return block;
        blockChanged = true;
        return { ...block, text: compressed };
      });
      if (!blockChanged) return message;
      changed = true;
      return { ...message, content: nextContent };
    }
    return message;
  });
  return changed ? { messages: next, changed } : { messages, changed: false };
}
