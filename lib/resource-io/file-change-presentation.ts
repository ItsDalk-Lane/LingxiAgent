import { isUtf8 } from "node:buffer";
import { createTwoFilesPatch } from "diff";
import type { ToolFileChangePresentation } from "../../shared/tool-presentation.ts";

// 展示记录只保留有界正文；差异计算另设时间和行数限制，避免拖住实际写入。
export const FILE_PRESENTATION_MAX_BYTES = 256 * 1024;
const FILE_DIFF_MAX_LINES = 10_000;

export type BeforeFileContent = { text?: string; reason?: string };

export function previousFileContent(content: Buffer): BeforeFileContent {
  if (content.byteLength > FILE_PRESENTATION_MAX_BYTES) {
    return { reason: "before_content_too_large" };
  }
  if (!isUtf8(content) || content.includes(0)) {
    return { reason: "before_content_not_text" };
  }
  return { text: content.toString("utf-8") };
}

function boundedContent(text: string): { content: string; truncated?: true } {
  if (Buffer.byteLength(text, "utf-8") <= FILE_PRESENTATION_MAX_BYTES) return { content: text };
  // 先按字符取有界前缀，避免为大文件再分配一整份 Buffer。
  const bytes = Buffer.from(text.slice(0, FILE_PRESENTATION_MAX_BYTES), "utf-8");
  let end = FILE_PRESENTATION_MAX_BYTES;
  // 不把 UTF-8 字符截成无效字节。
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end--;
  return { content: bytes.subarray(0, end).toString("utf-8"), truncated: true };
}

export function retainAppliedPatch(
  presentation: ToolFileChangePresentation,
  patch: unknown,
): ToolFileChangePresentation {
  if (typeof patch !== "string") return presentation;
  if (Buffer.byteLength(patch, "utf-8") > FILE_PRESENTATION_MAX_BYTES) {
    return { ...presentation, truncated: true, reason: "patch_too_large" };
  }
  return { ...presentation, patch };
}

export function fileChangePresentation({
  filePath,
  content,
  before,
  changeType,
  generatePatch,
}: {
  filePath: string;
  content: string | Buffer;
  before: BeforeFileContent;
  changeType?: "created" | "modified";
  generatePatch: boolean;
}): ToolFileChangePresentation {
  const text = typeof content === "string" ? content : content.toString("utf-8");
  // 只有写入结果确认新建时，才可以把原内容视为空；读失败不能证明文件不存在。
  const original = changeType === "created" ? "" : before.text;
  const presentation: ToolFileChangePresentation = {
    path: filePath,
    ...boundedContent(text),
    beforeAvailable: original !== undefined,
    ...(changeType ? { changeType } : {}),
    ...(original === undefined ? { reason: before.reason || "before_content_unavailable" } : {}),
  };
  if (!generatePatch || original === undefined) return presentation;
  if (presentation.truncated || original.split("\n").length + text.split("\n").length > FILE_DIFF_MAX_LINES) {
    return { ...presentation, truncated: true, reason: "diff_too_large" };
  }
  try {
    const patch = createTwoFilesPatch(filePath, filePath, original, text, undefined, undefined, {
      context: 3,
      timeout: 100,
      maxEditLength: FILE_DIFF_MAX_LINES,
    });
    return patch === undefined
      ? { ...presentation, truncated: true, reason: "diff_timeout" }
      : retainAppliedPatch(presentation, patch);
  } catch {
    return { ...presentation, reason: "diff_unavailable" };
  }
}
