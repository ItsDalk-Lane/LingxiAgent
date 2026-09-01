export const ALLOWED_CHAT_VIDEO_MIME_TYPES = Object.freeze([
  "video/mp4",
  "video/webm",
  "video/quicktime",
]);

export const MAX_CHAT_VIDEO_BASE64_CHARS = 20 * 1024 * 1024;
/** Base64 约膨胀为原文件的 4/3；编码前先用这个边界拒绝。 */
export const MAX_CHAT_VIDEO_SOURCE_BYTES = Math.floor(MAX_CHAT_VIDEO_BASE64_CHARS * 3 / 4);

const MIME_TO_EXT: Readonly<Record<string, string>> = Object.freeze({
  "video/mp4": ".mp4",
  "video/webm": ".webm",
  "video/quicktime": ".mov",
});

export function normalizeVideoMimeType(mimeType: unknown): string {
  return typeof mimeType === "string" ? mimeType.trim().toLowerCase() : "";
}

export function isAllowedChatVideoMime(mimeType: unknown): boolean {
  return ALLOWED_CHAT_VIDEO_MIME_TYPES.includes(normalizeVideoMimeType(mimeType));
}

export function extensionFromChatVideoMime(mimeType: unknown): string {
  return MIME_TO_EXT[normalizeVideoMimeType(mimeType)] || "";
}

export function isChatVideoBase64WithinLimit(base64Data: unknown): base64Data is string {
  return typeof base64Data === "string" && base64Data.length <= MAX_CHAT_VIDEO_BASE64_CHARS;
}

/** 浏览器、WebSocket 和服务端共用的内容校验；只解码文件头，不复制整段视频。 */
export function isChatVideoBase64ContentCompatible(base64Data: unknown, mimeType: unknown): boolean {
  if (typeof base64Data !== "string" || !base64Data) return false;
  try {
    const binary = globalThis.atob(base64Data.slice(0, 24));
    const bytes = Uint8Array.from(binary, character => character.charCodeAt(0));
    return isChatVideoBytesCompatible(bytes, mimeType);
  } catch {
    return false;
  }
}

/** 上传边界的轻量魔数校验，拒绝把任意文件只改扩展名后伪装成视频。 */
export function isChatVideoBytesCompatible(bytes: ArrayLike<number> | null | undefined, mimeType: unknown): boolean {
  if (!bytes || typeof bytes.length !== "number") return false;
  const mime = normalizeVideoMimeType(mimeType);
  if (mime === "video/webm") {
    return bytes.length >= 4
      && bytes[0] === 0x1a && bytes[1] === 0x45 && bytes[2] === 0xdf && bytes[3] === 0xa3;
  }
  if (mime === "video/mp4" || mime === "video/quicktime") {
    return bytes.length >= 12
      && bytes[4] === 0x66 && bytes[5] === 0x74 && bytes[6] === 0x79 && bytes[7] === 0x70;
  }
  return false;
}
