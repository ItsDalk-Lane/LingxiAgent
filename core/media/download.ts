import fs from "fs";
import path from "path";
import crypto from "crypto";

const MIME_TO_EXT = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/webp": "webp",
  // 语音合成产物（openai-audio-speech / system-speech）
  "audio/mpeg": "mp3",
  "audio/ogg": "ogg",
  "audio/aac": "aac",
  "audio/flac": "flac",
  "audio/wav": "wav",
  "audio/mp4": "m4a",
};

/**
 * Save image buffer to disk.
 * @param {Buffer} buffer
 * @param {string} mimeType
 * @param {string} dataDir - plugin data directory (ctx.dataDir)
 * @param {string} [customName] - optional filename without extension (e.g. "sunset-cat")
 * @returns {Promise<{ filename: string, filePath: string }>}
 */
export async function saveImage(buffer, mimeType, dataDir, customName) {
  const ext = MIME_TO_EXT[mimeType] || "png";
  // 未知 MIME 仍回落 png（历史行为）；音频适配器只传上表内的 MIME。
  const hash = crypto.createHash("md5").update(buffer).digest("hex").slice(0, 8);
  // sanitize custom name: keep alphanumeric, CJK, hyphens, underscores
  const safeName = customName
    ? customName.replace(/[^\w\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff-]/g, "_").slice(0, 80)
    : null;
  const filename = safeName
    ? `${safeName}-${hash}.${ext}`
    : `${Date.now()}-${hash}.${ext}`;
  const dir = path.join(dataDir, "generated");
  fs.mkdirSync(dir, { recursive: true });
  const filePath = path.join(dir, filename);
  await fs.promises.writeFile(filePath, buffer);
  return { filename, filePath };
}
