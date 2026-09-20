import fs from "fs";
import path from "path";
import { randomBytes } from "crypto";
import { atomicWriteSync } from "../shared/safe-fs.ts";

/**
 * 改前存档仓库：每个存档一份完整复印件（JSON 单文件）。
 *
 * 覆盖范围与工作区快照对齐：任何文件类型都收（二进制走 base64 保真），
 * 无大小上限；maxSizeKb 仅作为可选约束保留给显式传入的调用方。
 * 会话删除时由 purgeSession 按 sessionPath 归属清理。
 */
export class CheckpointStore {
  declare _dir: string;

  constructor(checkpointsDir: string) {
    this._dir = checkpointsDir;
  }

  async save({ sessionPath, tool, filePath, maxSizeKb = null, source, reason }: { sessionPath: string; tool: string; filePath: string; maxSizeKb?: number | null; source: string; reason: string }) {
    let stat;
    try {
      stat = fs.statSync(filePath);
    } catch {
      return null;
    }

    // 大小上限可选：未传或非正数时不限制（与工作区快照一致，任何大小的文件都收）
    if (typeof maxSizeKb === "number" && maxSizeKb > 0 && stat.size > maxSizeKb * 1024) return null;

    const buf = fs.readFileSync(filePath);
    // 全量 round-trip 判断是否为合法 utf-8 文本：损坏字节序列经替换字符
    // 往返后与原始字节不同，判为二进制，走 base64 存储；文本继续内嵌字符串。
    const roundTrip = Buffer.from(buf.toString("utf-8"), "utf-8");
    const isText = roundTrip.length === buf.length && roundTrip.equals(buf);

    fs.mkdirSync(this._dir, { recursive: true });
    const ts = Date.now();
    const suffix = randomBytes(2).toString("hex");
    const id = `${ts}_${suffix}`;
    const filename = `${id}.json`;
    const fileFull = path.join(this._dir, filename);

    const data = JSON.stringify({
      ts,
      sessionPath: sessionPath || null,
      tool,
      source: source || "llm",
      reason: reason || `tool-${tool}`,
      path: filePath,
      ...(isText ? { content: buf.toString("utf-8") } : { content_b64: buf.toString("base64") }),
      size: stat.size,
    });

    atomicWriteSync(fileFull, data);

    return id;
  }

  async list() {
    let entries;
    try {
      entries = fs.readdirSync(this._dir);
    } catch {
      return [];
    }

    const results = [];
    for (const name of entries) {
      if (!name.endsWith(".json") || name.endsWith(".tmp")) continue;
      try {
        const raw = fs.readFileSync(path.join(this._dir, name), "utf-8");
        const obj = JSON.parse(raw);
        results.push({
          id: name.replace(/\.json$/, ""),
          ts: obj.ts,
          tool: obj.tool,
          source: obj.source || "llm",
          reason: obj.reason || `tool-${obj.tool}`,
          path: obj.path,
          size: obj.size,
          // 会话归属：快照拍照失败时按 sessionPath + ts 倒推该轮的改前备份
          sessionPath: typeof obj.sessionPath === "string" ? obj.sessionPath : null,
        });
      } catch {
        // corrupted file, skip
      }
    }

    results.sort((a, b) => b.ts - a.ts);
    return results;
  }

  async restore(id: string) {
    const filePath = path.join(this._dir, `${id}.json`);
    const raw = fs.readFileSync(filePath, "utf-8");
    const obj = JSON.parse(raw);

    fs.mkdirSync(path.dirname(obj.path), { recursive: true });
    // 二进制存档（base64）无损写回；旧格式与文本存档继续用 utf-8 字符串。
    if (typeof obj.content_b64 === "string" && obj.content_b64) {
      fs.writeFileSync(obj.path, Buffer.from(obj.content_b64, "base64"));
    } else {
      fs.writeFileSync(obj.path, obj.content, "utf-8");
    }

    return { restoredTo: obj.path };
  }

  async remove(id: string) {
    const filePath = path.join(this._dir, `${id}.json`);
    try {
      fs.unlinkSync(filePath);
    } catch {}
  }

  /** 会话删除时按归属清理：只删 sessionPath 完全匹配的存档，不动手动编辑等无会话归属的存档 */
  async purgeSession(sessionPath: string) {
    if (typeof sessionPath !== "string" || !sessionPath) return { purged: 0 };
    let purged = 0;
    for (const entry of await this.list()) {
      if (entry.sessionPath !== sessionPath) continue;
      await this.remove(entry.id);
      purged += 1;
    }
    return { purged };
  }

  async cleanup(retentionDays: number) {
    let entries;
    try {
      entries = fs.readdirSync(this._dir);
    } catch {
      return;
    }

    const cutoff = Date.now() - retentionDays * 24 * 60 * 60 * 1000;
    for (const name of entries) {
      if (!name.endsWith(".json") || name.endsWith(".tmp")) continue;
      const ts = parseInt(name.split("_")[0], 10);
      if (!isNaN(ts) && ts < cutoff) {
        try {
          fs.unlinkSync(path.join(this._dir, name));
        } catch {}
      }
    }
  }
}
