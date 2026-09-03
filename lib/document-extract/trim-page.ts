import { setImmediate } from "node:timers/promises";

/** 只去掉扫描页外围纯白空白，不缩放、不阈值化、不删除浅色文字。 */
export async function trimRenderedPage(image: Uint8Array, signal?: AbortSignal): Promise<Uint8Array> {
  signal?.throwIfAborted();
  const { createCanvas, loadImage } = await import("@napi-rs/canvas");
  const decoded = await loadImage(Buffer.from(image));
  const { width, height } = decoded;
  const canvas = createCanvas(width, height);
  const context = canvas.getContext("2d");
  context.drawImage(decoded, 0, 0);
  const { data } = context.getImageData(0, 0, width, height);
  let left = width, right = -1, top = height, bottom = -1;
  for (let y = 0; y < height; y++) {
    if (y % 64 === 0) { await setImmediate(); signal?.throwIfAborted(); }
    for (let x = 0; x < width; x++) {
      const offset = (y * width + x) * 4;
      if (data[offset] === 255 && data[offset + 1] === 255 && data[offset + 2] === 255 && data[offset + 3] === 255) continue;
      left = Math.min(left, x); right = Math.max(right, x);
      top = Math.min(top, y); bottom = Math.max(bottom, y);
    }
  }
  if (right < left) return image;
  // 留出小段边距，只有明显减少无用像素时才重新编码。
  left = Math.max(0, left - 16); top = Math.max(0, top - 16);
  right = Math.min(width - 1, right + 16); bottom = Math.min(height - 1, bottom + 16);
  const croppedWidth = right - left + 1, croppedHeight = bottom - top + 1;
  if (croppedWidth * croppedHeight >= width * height * 0.8) return image;
  const cropped = createCanvas(croppedWidth, croppedHeight);
  cropped.getContext("2d").putImageData(context.getImageData(left, top, croppedWidth, croppedHeight), 0, 0);
  signal?.throwIfAborted();
  return cropped.toBuffer("image/png");
}
