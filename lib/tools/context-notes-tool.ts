/**
 * context-notes-tool.ts — 跨压缩的上下文笔记（阶段二·11）。
 *
 * 笔记是「模型写给未来自己」的便签：约束、决定、关键坐标——压缩时
 * 旧区会被摘要掉，但压缩器会把笔记全文重新注入摘要尾部（照 skill-recall
 * / plan-file 同一保护族），所以跨压缩存活。落盘在会话旁
 * `${sessionPath}.context-notes.md`（纯 markdown，不占消息流，不进回档）。
 * 16KiB 上限（写超拒绝，append 溢出拒绝）；read=read 权限，
 * write/append=write（只读档拦写、留读）。
 */
import fs from "node:fs";
import path from "node:path";
import { Type, StringEnum } from "../pi-sdk/index.ts";

export const CONTEXT_NOTES_MAX_BYTES = 16 * 1024;
export const CONTEXT_NOTES_TRUNCATION_MARKER = "\n…(context notes truncated at 16KiB cap)";

export function contextNotesPath(sessionPath: string): string {
  return `${sessionPath}.context-notes.md`;
}

export function readContextNotes(sessionPath: string): string {
  try {
    const text = fs.readFileSync(contextNotesPath(sessionPath), "utf8");
    if (Buffer.byteLength(text, "utf8") > CONTEXT_NOTES_MAX_BYTES) {
      // 上限由写入侧保证；这里兜底截断保证消费方永不超载
      return Buffer.from(text, "utf8").subarray(0, CONTEXT_NOTES_MAX_BYTES).toString("utf8") + CONTEXT_NOTES_TRUNCATION_MARKER;
    }
    return text;
  } catch {
    return "";
  }
}

export function writeContextNotes(sessionPath: string, text: string): void {
  const target = contextNotesPath(sessionPath);
  const tmp = `${target}.tmp-${process.pid}-${Date.now()}`;
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(tmp, text);
  fs.renameSync(tmp, target);
}

export function createContextNotesTool(deps: { getSessionPath: () => string | null }) {
  return {
    name: "context_notes",
    description: "Notes to your future self that survive context compaction: constraints, decisions, key file paths, gotchas. Keep them terse and current — stale notes mislead. The compactor re-injects the full notes at the end of every summary, so anything important for the rest of this task belongs here, not in chat scrollback. action=read shows current notes; write replaces; append adds to the end.",
    parameters: Type.Object({
      action: StringEnum(["read", "write", "append"], { description: "read: show current notes (default). write: replace all notes. append: add a block to the end" }),
      text: Type.String({ description: "Note text for write/append (markdown; keep it short — 16KiB cap total)" }),
    }),
    sessionPermission: {
      resolveInvocation: (input: any = {}) => {
        if (input?.action === "read" || input?.action == null) {
          return { action: "read", kind: "read", capability: "context_notes.read" };
        }
        return { action: input.action === "append" ? "append" : "write", kind: "write", capability: "context_notes.write" };
      },
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      const sessionPath = deps.getSessionPath?.() || null;
      if (!sessionPath) {
        return { isError: true, content: [{ type: "text", text: "no active session — context notes are session-scoped" }] };
      }
      const action = params?.action === "write" || params?.action === "append" ? params.action : "read";

      if (action === "read") {
        const notes = readContextNotes(sessionPath);
        return {
          content: [{ type: "text", text: notes ? `context notes:\n${notes}` : "(no context notes yet)" }],
          details: { bytes: Buffer.byteLength(notes, "utf8"), capBytes: CONTEXT_NOTES_MAX_BYTES },
        };
      }

      const text = typeof params?.text === "string" ? params.text : "";
      if (action === "write" && !text.trim()) {
        return { content: [{ type: "text", text: "refusing to write empty notes (use them or drop them — delete via write with a space is not supported)" }] };
      }
      if (!text.trim()) {
        return { content: [{ type: "text", text: "text is required for append" }] };
      }
      const current = readContextNotes(sessionPath);
      const next = action === "append"
        ? (current ? `${current.replace(/\s*$/, "")}\n${text}` : text)
        : text;
      const bytes = Buffer.byteLength(next, "utf8");
      if (bytes > CONTEXT_NOTES_MAX_BYTES) {
        return {
          isError: true,
          content: [{
            type: "text",
            text: `context notes would exceed the ${Math.round(CONTEXT_NOTES_MAX_BYTES / 1024)}KiB cap `
              + `(${bytes} bytes). Prune stale entries first (read, then write a tightened version).`,
          }],
          details: { errorCode: "CONTEXT_NOTES_OVERSIZE", bytes, capBytes: CONTEXT_NOTES_MAX_BYTES },
        };
      }
      writeContextNotes(sessionPath, next);
      return {
        content: [{ type: "text", text: `context notes ${action === "append" ? "appended" : "written"} (${bytes}/${CONTEXT_NOTES_MAX_BYTES} bytes used). They survive compaction.` }],
        details: { action, bytes },
      };
    },
  };
}
