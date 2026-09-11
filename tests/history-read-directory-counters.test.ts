/**
 * A05 仪表计数器自测（不依赖大夹具）：
 * 证明 ①整文件读（read API 与 SDK 式 openSync+readSync 顺序 episode 两种形态）、
 * ②定位短读（explicit position 短读/EOF）、③JSONL 行级解析（增量解析入口）
 * 都能被正确计数；并验证请求作用域隔离与卸载恢复。
 *
 * 运行环境：vitest（node 环境即可）；包装为进程内全局补丁，install/uninstall 严格配对，
 * 卸载后不得影响同 worker 内的其他测试。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import {
  createHistoryReadCounters,
  getActiveHistoryReadCounters,
} from "../scripts/lib/history-read-counters.mjs";
import * as instrToolOutcome from "../scripts/lib/instr-tool-outcome.mjs";
import {
  buildLongRunFixtureBytes,
  sha256,
  SESSION_FILE_HEADER_ID,
} from "../scripts/lib/history-read-fixture.mjs";

let counters: any = null;
let handle: any = null;

function install(label: string, sessionPath: string, meta: Record<string, unknown> = {}) {
  counters = createHistoryReadCounters({ label });
  const originalReadFileSync = fs.readFileSync;
  counters.originalReadFileSync = originalReadFileSync;
  counters.install();
  handle = counters.beginRequest({ sessionPath, requestKind: "self-test", ...meta });
  return handle;
}

afterEach(() => {
  if (handle) {
    counters.endRequest(handle);
    handle = null;
  }
  if (counters) {
    counters.uninstall();
    expect(fs.readFileSync).toBe(counters.originalReadFileSync);
    counters = null;
  }
});

function writeTempSession(lines: number): { dir: string; sessionPath: string } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-counter-test-"));
  const sessionPath = path.join(dir, "hana", "sessions", "counter-target.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  // 用 A03 夹具的前 N 行切片 + 文件头，保证每行都是会话形态（{"type":...）
  const fixtureLines = buildLongRunFixtureBytes(Math.ceil(lines / 2))
    .toString("utf8")
    .split("\n");
  const selected = [fixtureLines[0], ...fixtureLines.slice(1, lines)];
  fs.writeFileSync(sessionPath, selected.join("\n"), "utf8");
  return { dir, sessionPath };
}

describe("history-read 仪表计数器", () => {
  it("C1：readFileSync / promises.readFile 整文件读计入 fullFileReadCalls（read-api 形态）", async () => {
    const { dir, sessionPath } = writeTempSession(30);
    try {
      const size = fs.statSync(sessionPath).size;
      const h = install("c1", sessionPath);
      handle.run(() => {
        fs.readFileSync(sessionPath, "utf-8");
      });
      await handle.run(() => fsp.readFile(sessionPath, "utf-8"));
      const rec = counters.endRequest(handle);
      handle = null;
      const g = counters.snapshotOf(rec.gauges);
      expect(g.fullFileReadCalls).toBe(2);
      expect(g.fullFileReadViaReadApi).toBe(2);
      expect(g.fullFileReadViaFdEpisode).toBe(0);
      expect(g.sessionFullFileReadCalls).toBe(2);
      expect(g.readCalls).toBe(2);
      expect(g.sessionFileReadBytes).toBe(size * 2);
      expect(g.logicalReadBytes).toBe(size * 2);
      expect(g.readMs).toBeGreaterThanOrEqual(0);
      fs.rmSync(dir, { recursive: true, force: true });
    } catch (e) {
      fs.rmSync(dir, { recursive: true, force: true });
      throw e;
    }
  });

  it("C2：SDK 式 openSync+readSync 顺序全量 episode 在 close 时计一次整文件读（fd-episode 形态）", () => {
    const { dir, sessionPath } = writeTempSession(64);
    try {
      const size = fs.statSync(sessionPath).size;
      const h = install("c2", sessionPath);
      handle.run(() => {
        const fd = fs.openSync(sessionPath, "r");
        const buffer = Buffer.alloc(1024);
        for (;;) {
          const bytesRead = fs.readSync(fd, buffer, 0, buffer.length, null);
          if (bytesRead === 0) break;
        }
        fs.closeSync(fd);
      });
      const rec = counters.endRequest(handle);
      handle = null;
      const g = counters.snapshotOf(rec.gauges);
      // 最后一次 0 字节 EOF 读 + 中途短读：1MiB<1024 不成立，这里是文件小于 buffer 的短读
      expect(g.fullFileReadCalls).toBe(1);
      expect(g.fullFileReadViaFdEpisode).toBe(1);
      expect(g.fullFileReadViaReadApi).toBe(0);
      expect(g.sessionFullFileReadCalls).toBe(1);
      expect(g.logicalReadBytes).toBe(size);
      expect(g.eofReads).toBe(1);
      fs.rmSync(dir, { recursive: true, force: true });
    } catch (e) {
      fs.rmSync(dir, { recursive: true, force: true });
      throw e;
    }
  });

  it("C3：定位短读（explicit position）不计整文件读，短读/EOF 分别计数", () => {
    const { dir, sessionPath } = writeTempSession(64);
    try {
      const size = fs.statSync(sessionPath).size;
      const h = install("c3", sessionPath);
      handle.run(() => {
        const fd = fs.openSync(sessionPath, "r");
        const buffer = Buffer.alloc(1024);
        // 从文件中部起读：非顺序覆盖 [0,size)，不构成整文件读
        const mid = fs.readSync(fd, buffer, 0, 10, Math.floor(size / 2));
        expect(mid).toBe(10);
        // 从文件尾外读：bytesRead=0
        const past = fs.readSync(fd, buffer, 0, 16, size + 100);
        expect(past).toBe(0);
        // 尾部短读：请求 64 字节、只剩少量字节
        const tail = fs.readSync(fd, buffer, 0, 64, size - 5);
        expect(tail).toBe(5);
        fs.closeSync(fd);
      });
      const rec = counters.endRequest(handle);
      handle = null;
      const g = counters.snapshotOf(rec.gauges);
      expect(g.fullFileReadCalls).toBe(0);
      expect(g.readCalls).toBe(3);
      expect(g.shortReadEvents).toBe(1); // 请求 64 实得 5
      expect(g.eofReads).toBe(1); // 0 字节
      expect(g.logicalReadBytes).toBe(15);
      fs.rmSync(dir, { recursive: true, force: true });
    } catch (e) {
      fs.rmSync(dir, { recursive: true, force: true });
      throw e;
    }
  });

  it("C4：JSONL 行级 JSON.parse 计数——会话形态行计入、非会话 JSON 不计入", () => {
    const { dir, sessionPath } = writeTempSession(8);
    try {
      const h = install("c4", sessionPath);
      handle.run(() => {
        JSON.parse('{"type":"message","id":"u1","parentId":null,"message":{"role":"user"}}');
        JSON.parse('{"type":"session","version":3,"id":"x"}');
        JSON.parse('{"hello":"world"}'); // 非会话形态
        expect(() => JSON.parse({ a: 1 } as any)).toThrow(); // 非字符串输入照常抛错，不计数
      });
      const rec = counters.endRequest(handle);
      handle = null;
      const g = counters.snapshotOf(rec.gauges);
      expect(g.jsonlParseCount).toBe(2);
      expect(g.parseMs).toBeGreaterThanOrEqual(g.jsonlParseMs);
      fs.rmSync(dir, { recursive: true, force: true });
    } catch (e) {
      fs.rmSync(dir, { recursive: true, force: true });
      throw e;
    }
  });

  it("C5：请求作用域隔离——两次请求各自归因，作用域外调用不进请求桶", async () => {
    const { dir, sessionPath } = writeTempSession(8);
    try {
      const c = createHistoryReadCounters({ label: "c5" });
      c.install();
      const h1 = c.beginRequest({ sessionPath, requestKind: "r1" });
      h1.run(() => {
        fs.readFileSync(sessionPath, "utf-8");
      });
      const r1 = c.endRequest(h1);
      // 作用域外读取（不归属任何请求）
      fs.readFileSync(sessionPath, "utf-8");
      const h2 = c.beginRequest({ sessionPath, requestKind: "r2" });
      await h2.run(async () => {
        await fsp.readFile(sessionPath, "utf-8");
      });
      const r2 = c.endRequest(h2);
      const g1 = c.snapshotOf(r1.gauges);
      const g2 = c.snapshotOf(r2.gauges);
      expect(g1.readCalls).toBe(1);
      expect(g2.readCalls).toBe(1);
      expect(g2.fullFileReadCalls).toBe(1);
      // global（harness）桶只含作用域外那一次
      expect(c.global().readCalls).toBe(1);
      c.uninstall();
      fs.rmSync(dir, { recursive: true, force: true });
    } catch (e) {
      fs.rmSync(dir, { recursive: true, force: true });
      throw e;
    }
  });

  it("C6：会话文件写入计数（repair 重写旁证）与 FileHandle 读计数", async () => {
    const { dir, sessionPath } = writeTempSession(8);
    try {
      const h = install("c6", sessionPath);
      await handle.run(async () => {
        const fh = await fsp.open(sessionPath, "r");
        const buffer = Buffer.alloc(512);
        await fh.read(buffer, 0, buffer.length, 0);
        await fh.close();
        fs.writeFileSync(`${sessionPath}.probe`, "x");
      });
      const rec = counters.endRequest(handle);
      handle = null;
      const g = counters.snapshotOf(rec.gauges);
      expect(g.readCalls).toBe(1);
      expect(g.logicalReadBytes).toBe(512);
      expect(g.writeCalls).toBe(1); // .probe 文件（非会话路径，不进 sessionFileWriteCalls）
      expect(g.sessionFileWriteCalls).toBe(0);
      fs.rmSync(dir, { recursive: true, force: true });
    } catch (e) {
      fs.rmSync(dir, { recursive: true, force: true });
      throw e;
    }
  });

  it("C7：仪表包装模块（shared/tool-outcome collectToolOutcomesByCallId）计入 scanCalls 与 metadataVisited", () => {
    const { dir, sessionPath } = writeTempSession(8);
    try {
      const h = install("c7", sessionPath);
      const messages = [
        { role: "user", content: "hi" },
        { role: "assistant", content: [{ type: "tool_use", id: "tu-1", name: "read_file" }] },
        { role: "toolResult", toolCallId: "tu-1", content: "ok" },
      ];
      const outcomes = handle.run(() => instrToolOutcome.collectToolOutcomesByCallId(messages));
      expect(outcomes).toBeInstanceOf(Map);
      const rec = counters.endRequest(handle);
      handle = null;
      const g = counters.snapshotOf(rec.gauges);
      expect(g.scanCalls.collectToolOutcomesByCallId).toBeDefined();
      expect(g.scanCalls.collectToolOutcomesByCallId.calls).toBe(1);
      expect(g.scanCalls.collectToolOutcomesByCallId.entriesVisited).toBe(3);
      expect(g.metadataVisitedCount).toBe(3);
      expect(getActiveHistoryReadCounters()).toBeTruthy();
      fs.rmSync(dir, { recursive: true, force: true });
    } catch (e) {
      fs.rmSync(dir, { recursive: true, force: true });
      throw e;
    }
  });

  it("C8：A03 夹具构建器字节级一致（与 fixture-audit.json 记录同 sha256）", () => {
    const bytes = buildLongRunFixtureBytes(140);
    // A03 审计记录（artifacts/history-read-directory/fixture-audit.json，N=140）
    expect(bytes.length).toBe(61290);
    expect(sha256(bytes)).toBe(
      "86e5cecad75fb1a465514134a0cdfb7824ad1a85fbe3d9c283efc9a6d4c7050e",
    );
    const header = JSON.parse(bytes.toString("utf8").split("\n")[0]);
    expect(header.id).toBe(SESSION_FILE_HEADER_ID);
  });
});
