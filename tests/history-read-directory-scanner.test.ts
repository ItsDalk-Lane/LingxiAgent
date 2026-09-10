/**
 * B02 只读扫描器测试（X06 / X07 / 扫描层 X01）：
 *  - 尾换行有/无、CRLF、中文/emoji、跨块 UTF-8 半字符（随机多字节内容 × 随机 chunk
 *    边界 × 4 种行尾组合的性质测试：逐条 offset 读 === 整文件 parse）；
 *  - 注入短读多次补齐；提前 EOF 不读空 Buffer；
 *  - 坏尾行 → pendingTailOffset 不进条目、下次可从其起点续读；
 *  - 已完成位置坏行 → corrupt_record 停止、不写文件；
 *  - 超限行内存套用 projectOversizedSessionEntry（hanaRepair 形状与 repair 产物一致，
 *    不产生 .repair.json、不改文件字节）。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";

import { scanHistoryFile } from "../server/history-read/scan.ts";
import { projectOversizedSessionEntry, repairOversizedSessionEntriesInFile, DEFAULT_SESSION_JSONL_MAX_LINE_BYTES } from "../core/session-jsonl-file.ts";

const tmpDirs: string[] = [];

afterEach(() => {
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-scan-test-"));
  tmpDirs.push(dir);
  return dir;
}

function writeFixture(name: string, content: string | Buffer): string {
  const p = path.join(makeTmpDir(), name);
  fs.writeFileSync(p, content);
  return p;
}

async function scanFile(p: string, opts: Record<string, any> = {}) {
  return await scanHistoryFile(p, { capturedLength: fs.statSync(p).size, ...opts });
}

function expectedLineOffsets(lines: string[], sep: "\n" | "\r\n", trailingNewline: boolean) {
  const offsets: number[] = [];
  const sepLen = sep === "\r\n" ? 2 : 1;
  let cursor = 0;
  lines.forEach((line, i) => {
    offsets.push(cursor);
    cursor += Buffer.byteLength(line, "utf8") + (i < lines.length - 1 || trailingNewline ? sepLen : 0);
  });
  return { offsets, total: cursor };
}

function jsonlLine(id: string, parentId: string | null, message: any) {
  return JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message });
}

/** 逐条 offset 读 === 整文件 parse（X06 性质断言）。 */
function expectOffsetReadsMatchWholeFile(p: string, result: ReturnType<() => any>) {
  const raw = fs.readFileSync(p);
  const wholeEntries = raw.toString("utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l));
  expect(result.entries.length).toBe(wholeEntries.length);
  result.physical.forEach((phys: any, i: number) => {
    const slice = raw.subarray(phys.byteOffset, phys.byteOffset + phys.byteLength);
    expect(JSON.parse(slice.toString("utf8"))).toEqual(wholeEntries[i]);
  });
  expect(result.entries).toEqual(wholeEntries);
}

describe("scanHistoryFile 行尾/编码/分块", () => {
  const LINES = [
    JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" }),
    jsonlLine("u1", null, { role: "user", content: "请完成长任务并给出最终报告 🎯" }),
    jsonlLine("a1", "u1", { role: "assistant", content: [{ type: "thinking", thinking: "思考 🤔 片段" }, { type: "text", text: "中文内容——含 emoji ✅ 与 ASCII abc。" }] }),
  ];

  for (const sep of ["\n", "\r\n"] as const) {
    for (const trailingNewline of [true, false]) {
      it(`${sep === "\r\n" ? "CRLF" : "LF"} ${trailingNewline ? "带" : "无"}尾换行：偏移/分隔符/条目全部正确（跨块 7 字节）`, async () => {
        const content = LINES.join(sep) + (trailingNewline ? sep : "");
        const p = writeFixture("fixture.jsonl", content);
        const result = await scanFile(p, { chunkBytes: 7 });

        const { offsets, total } = expectedLineOffsets(LINES, sep, trailingNewline);
        expect(result.error).toBeNull();
        expect(result.entries).toEqual(LINES.map((l) => JSON.parse(l)));
        expect(result.physical.length).toBe(LINES.length);
        result.physical.forEach((phys: any, i: number) => {
          expect(phys.physicalIndex).toBe(i);
          expect(phys.byteOffset).toBe(offsets[i]);
          expect(phys.byteLength).toBe(Buffer.byteLength(LINES[i], "utf8"));
          expect(phys.separatorLength).toBe(i < LINES.length - 1 || trailingNewline ? (sep === "\r\n" ? 2 : 1) : 0);
        });
        expect(result.bounds.observedFileSize).toBe(total);
        expect(result.bounds.indexedThroughOffset).toBe(trailingNewline ? total : total);
        expect(result.bounds.pendingTailOffset).toBeNull();
        expect(result.bounds.lastUndelimitedRow).toEqual(
          trailingNewline ? null : { offset: offsets[LINES.length - 1], length: Buffer.byteLength(LINES[LINES.length - 1], "utf8") },
        );
        expect(result.header).toEqual({ sdkSessionId: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", version: 3 });
        expectOffsetReadsMatchWholeFile(p, result);
      });
    }
  }

  it("性质测试：随机多字节内容 × 随机 chunk 边界 × 4 种行尾组合，逐条 offset 读===整文件 parse", async () => {
    // 固定种子伪随机（可复现，不依赖 Math.random 顺序）
    let seed = 20260910;
    const rand = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 0xffffffff;
    };
    const CJK = "中文 emoji ✅🎯 测试 𝒜𝒷 multibyte 日本語";

    for (let iteration = 0; iteration < 24; iteration += 1) {
      const lineCount = 1 + Math.floor(rand() * 8);
      const lines: string[] = [];
      for (let i = 0; i < lineCount; i += 1) {
        const text = Array.from({ length: 1 + Math.floor(rand() * 6) }, () => CJK[Math.floor(rand() * CJK.length)]).join("");
        const entry = i === 0
          ? { type: "session", version: 3, id: "hdr", cwd: text }
          : jsonlLine(`id-${i}`, i === 1 ? null : `id-${i - 1}`, { role: rand() > 0.5 ? "user" : "assistant", content: text });
        lines.push(JSON.stringify(entry));
      }
      const sep = rand() > 0.5 ? "\r\n" : "\n";
      const trailingNewline = rand() > 0.5;
      const content = lines.join(sep) + (trailingNewline ? sep : "");
      const p = writeFixture(`prop-${iteration}.jsonl`, content);
      const chunkBytes = 1 + Math.floor(rand() * 97);
      const result = await scanFile(p, { chunkBytes });

      expect(result.error).toBeNull();
      const { offsets } = expectedLineOffsets(lines, sep, trailingNewline);
      const sepLen = sep === "\r\n" ? 2 : 1;
      result.physical.forEach((phys: any, i: number) => {
        expect(phys.byteOffset).toBe(offsets[i]);
        expect(phys.byteLength).toBe(Buffer.byteLength(lines[i], "utf8"));
        expect(phys.separatorLength).toBe(i < lines.length - 1 || trailingNewline ? sepLen : 0);
      });
      expectOffsetReadsMatchWholeFile(p, result);
    }
  });

  it("空行跳过且偏移不错位", async () => {
    const l1 = jsonlLine("u1", null, { role: "user", content: "你好" });
    const l2 = jsonlLine("a1", "u1", { role: "assistant", content: "完成" });
    const p = writeFixture("blank.jsonl", `\n${l1}\n\n\n${l2}\n`);
    const result = await scanFile(p);
    expect(result.error).toBeNull();
    expect(result.entries.length).toBe(2);
    const raw = fs.readFileSync(p);
    expect(result.physical[0].byteOffset).toBe(1);
    expect(result.physical[1].byteOffset).toBe(1 + Buffer.byteLength(l1, "utf8") + 3);
    expect(JSON.parse(raw.subarray(result.physical[1].byteOffset, result.physical[1].byteOffset + result.physical[1].byteLength).toString("utf8"))).toEqual(JSON.parse(l2));
  });
});

describe("scanHistoryFile 短读与 EOF（X07）", () => {
  it("注入短读：每次最多返回 5 字节，多次补齐后结果与整读一致", async () => {
    const lines = [
      JSON.stringify({ type: "session", version: 3, id: "hdr", cwd: "/tmp" }),
      jsonlLine("u1", null, { role: "user", content: "短读补齐测试 🎯 中文" }),
      jsonlLine("a1", "u1", { role: "assistant", content: "done" }),
    ];
    const content = lines.join("\n") + "\n";
    const p = writeFixture("short-read.jsonl", content);
    const raw = fs.readFileSync(p);

    let calls = 0;
    const shortRead = async (buffer: Buffer, offset: number, length: number, position: number) => {
      calls += 1;
      const n = Math.min(length, 5);
      if (position >= raw.length) return 0;
      const copied = raw.subarray(position, position + n).copy(buffer, offset);
      return copied;
    };

    const result = await scanHistoryFile(p, { capturedLength: raw.length, readFile: shortRead });
    expect(calls).toBeGreaterThanOrEqual(Math.ceil(raw.length / 5));
    const whole = await scanFile(p);
    expect(result.entries).toEqual(whole.entries);
    expect(result.physical).toEqual(whole.physical);
    expect(result.bounds).toEqual(whole.bounds);
  });

  it("提前 EOF：不读空白缓冲区，安全停在已收到的完整记录边界", async () => {
    const lines = [
      JSON.stringify({ type: "session", version: 3, id: "hdr", cwd: "/tmp" }),
      jsonlLine("u1", null, { role: "user", content: "第一" }),
      jsonlLine("a1", "u1", { role: "assistant", content: "第二" }),
    ];
    const content = lines.join("\n") + "\n";
    const cutoff = Buffer.byteLength(lines[0], "utf8") + 1 + Math.floor(Buffer.byteLength(lines[1], "utf8") / 2);
    const p = writeFixture("early-eof.jsonl", content);
    const raw = fs.readFileSync(p);
    const eofHook = async (buffer: Buffer, offset: number, length: number, position: number) => {
      if (position >= cutoff) return 0;
      return raw.subarray(position, Math.min(position + length, cutoff)).copy(buffer, offset);
    };

    const result = await scanHistoryFile(p, { capturedLength: raw.length, readFile: eofHook });
    expect(result.error).toBeNull();
    expect(result.entries.length).toBe(1); // 第二条只到一半 → 未完成行
    expect(result.bounds.observedFileSize).toBe(cutoff);
    expect(result.bounds.pendingTailOffset).toBe(
      Buffer.byteLength(lines[0], "utf8") + 1,
    );
    expect(result.bounds.indexedThroughOffset).toBe(
      Buffer.byteLength(lines[0], "utf8") + 1,
    );
  });
});

describe("scanHistoryFile 损坏与尾状态（I09）", () => {
  it("已完成位置坏行 → corrupt_record（physicalIndex 正确），前缀条目保留，不写文件", async () => {
    const good1 = jsonlLine("u1", null, { role: "user", content: "好行" });
    const bad = "{broken json";
    const good2 = jsonlLine("a1", "u1", { role: "assistant", content: "其后不再读" });
    const content = `${good1}\n${bad}\n${good2}\n`;
    const p = writeFixture("corrupt.jsonl", content);
    const before = fs.readFileSync(p);

    const result = await scanFile(p);
    expect(result.error).toEqual({ code: "corrupt_record", physicalIndex: 1, message: expect.any(String) });
    expect(result.entries.length).toBe(1);
    expect(result.entries[0].id).toBe("u1");
    expect(result.bounds.pendingTailOffset).toBeNull();
    expect(result.bounds.indexedThroughOffset).toBe(Buffer.byteLength(good1, "utf8") + 1);
    expect(fs.readFileSync(p).equals(before)).toBe(true);
  });

  it("坏尾行（截断 JSON、无换行）→ pendingTailOffset 不进条目；从其起点续读可补齐", async () => {
    const good = jsonlLine("u1", null, { role: "user", content: "前面完好" });
    const tailPrefix = `{"type":"message","id":"a1","parent`;
    const p = writeFixture("tail.jsonl", `${good}\n${tailPrefix}`);

    const first = await scanFile(p);
    expect(first.error).toBeNull();
    expect(first.entries.length).toBe(1);
    const tailOffset = Buffer.byteLength(good, "utf8") + 1;
    expect(first.bounds.pendingTailOffset).toBe(tailOffset);
    expect(first.bounds.indexedThroughOffset).toBe(tailOffset);
    expect(first.bounds.lastUndelimitedRow).toBeNull();

    // 追加补齐后半行 → 从 pendingTailOffset 续读，恰好多出一条且不重复
    fs.appendFileSync(p, `Id":"a1","parentId":"u1","message":{"role":"assistant","content":"补齐"}}\n`);
    const resumed = await scanHistoryFile(p, { capturedLength: fs.statSync(p).size, startOffset: first.bounds.pendingTailOffset });
    expect(resumed.error).toBeNull();
    expect(resumed.entries.length).toBe(1);
    expect(resumed.entries[0].id).toBe("a1");
    expect(resumed.physical[0].byteOffset).toBe(tailOffset);
    expect(resumed.bounds.pendingTailOffset).toBeNull();

    // 全量重扫与「续读 + 前缀」的条目集合一致
    const full = await scanFile(p);
    expect(full.entries.length).toBe(2);
    expect(full.entries[1]).toEqual(resumed.entries[0]);
  });

  it("不完整 UTF-8 尾（多字节字符中截断）→ pendingTailOffset", async () => {
    const good = jsonlLine("u1", null, { role: "user", content: "完好" });
    const truncatedEntry = Buffer.from(JSON.stringify({ type: "message", id: "a1", parentId: "u1", message: { role: "assistant", content: "中文截断🎯" } }), "utf8");
    const cut = truncatedEntry.length - 2; // 截掉 🎯 的后半（4 字节 emoji 的尾 2 字节）
    const p = writeFixture("utf8-tail.jsonl", Buffer.concat([Buffer.from(`${good}\n`, "utf8"), truncatedEntry.subarray(0, cut)]));

    const result = await scanFile(p);
    expect(result.error).toBeNull();
    expect(result.entries.length).toBe(1);
    expect(result.bounds.pendingTailOffset).toBe(Buffer.byteLength(good, "utf8") + 1);
    expect(result.bounds.indexedThroughOffset).toBe(result.bounds.pendingTailOffset);
  });

  it("无尾换行的完整末条 → lastUndelimitedRow 且正常纳入", async () => {
    const l1 = jsonlLine("u1", null, { role: "user", content: "有换行" });
    const l2 = jsonlLine("a1", "u1", { role: "assistant", content: "无换行末条" });
    const p = writeFixture("undelimited.jsonl", `${l1}\n${l2}`);
    const result = await scanFile(p);
    expect(result.error).toBeNull();
    expect(result.entries.length).toBe(2);
    expect(result.bounds.pendingTailOffset).toBeNull();
    expect(result.bounds.lastUndelimitedRow).toEqual({ offset: Buffer.byteLength(l1, "utf8") + 1, length: Buffer.byteLength(l2, "utf8") });
    expect(result.bounds.indexedThroughOffset).toBe(fs.statSync(p).size);
    expect(result.physical[1].separatorLength).toBe(0);
  });
});

describe("scanHistoryFile 超限行（内存投影，不写文件）", () => {
  it("超限行内存套用 projectOversizedSessionEntry：hanaRepair 形状与 repair 产物一致，无 .repair.json、字节不变", async () => {
    const bigText = "大".repeat(900);
    const oversizedLine = jsonlLine("a-big", "u1", { role: "assistant", content: bigText });
    const lines = [
      JSON.stringify({ type: "session", version: 3, id: "hdr", cwd: "/tmp" }),
      jsonlLine("u1", null, { role: "user", content: "正常行" }),
      oversizedLine,
    ];
    const content = lines.join("\n") + "\n";
    const p = writeFixture("oversized.jsonl", content);
    expect(Buffer.byteLength(oversizedLine, "utf8")).toBeGreaterThan(1024);
    const rawBefore = fs.readFileSync(p);
    const shaBefore = createHash("sha256").update(rawBefore).digest("hex");

    const result = await scanHistoryFile(p, { capturedLength: rawBefore.length, maxLineBytes: 1024 });
    expect(result.error).toBeNull();
    const bigPhys = result.physical[2];
    expect(bigPhys.oversized).toBe(true);
    expect(bigPhys.byteLength).toBe(Buffer.byteLength(oversizedLine, "utf8"));
    // 投影产物与 repair 链路的内存产物出自同一纯函数、同一入参 → 逐字段一致
    const expected = projectOversizedSessionEntry(JSON.parse(oversizedLine), {
      originalByteLength: Buffer.byteLength(oversizedLine, "utf8"),
      maxLineBytes: 1024,
    });
    expect(result.entries[2]).toEqual(expected);
    expect(result.entries[2].hanaRepair).toEqual({
      oversizedLineProjected: true,
      originalByteLength: Buffer.byteLength(oversizedLine, "utf8"),
    });
    // 正常行不受影响
    expect(result.physical[1].oversized).toBe(false);
    expect(result.entries[1].hanaRepair).toBeUndefined();

    // 不写文件、不产生 .repair.json
    const rawAfter = fs.readFileSync(p);
    expect(rawAfter.equals(rawBefore)).toBe(true);
    expect(createHash("sha256").update(rawAfter).digest("hex")).toBe(shaBefore);
    expect(fs.existsSync(`${p}.repair.json`)).toBe(false);

    // 对照：旧 repair 路径在同夹具副本上产出的条目与新扫描的内存投影一致
    const repairCopy = writeFixture("repair-copy.jsonl", content);
    const repairResult = repairOversizedSessionEntriesInFile(repairCopy, { maxLineBytes: 1024 });
    expect(repairResult.repaired).toBe(true);
    const repairedRaw = fs.readFileSync(repairCopy, "utf8");
    const repairedEntries = repairedRaw.split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l));
    expect(repairedEntries[2]).toEqual(result.entries[2]);
  });

  it("默认阈值 1 MiB 内的行不投影", async () => {
    const p = writeFixture("normal.jsonl", `${jsonlLine("u1", null, { role: "user", content: "小行" })}\n`);
    const result = await scanFile(p);
    expect(result.physical[0].oversized).toBe(false);
    expect(result.entries[0].hanaRepair).toBeUndefined();
    expect(DEFAULT_SESSION_JSONL_MAX_LINE_BYTES).toBe(1024 * 1024);
  });
});
