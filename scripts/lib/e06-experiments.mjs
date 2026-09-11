/**
 * e06-experiments.mjs — E06.1 逐候选聚合、压力场景与 etag 随 limit 变化校验。
 *
 * 从协议基准的逐请求记录聚合每候选（K=50/100/150/200）：
 *  - 首屏客户端时延（p50/p95/max，跨 7 链路格）
 *  - 热页客户端时延（翻页除首页外，p50/p95/max）与 JSON 解码
 *  - 每页平均正文 KiB、单页新增峰值内存增量（heap/external/arrayBuffers 分开）
 *  - 各链路格的 walk 请求数/总时/总字节
 * 压力场景覆盖：大正文/工具行、多块、多 Run、单超长 Run、隐藏整页、非局部依赖
 * （每场景：冷构建 + 热页一屏；热页解析数应远小于冷构建，即窗口受限）。
 * etag 随 limit 变化：同页不同 limit 的 ETag 必须不同（请求身份含 limit）。
 */
import fs from "node:fs";
import path from "node:path";
import { serve } from "@hono/node-server";

const HEADER = (id) => JSON.stringify({ type: "session", version: 3, id, cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" });
const msg = (id, parentId, message) => JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message });
const custom = (id, parentId, customType, data, extra = {}) =>
  JSON.stringify({ type: "custom", id, parentId, timestamp: "2026-09-10T10:00:00Z", customType, data, ...extra });

const range = (n, fn) => Array.from({ length: n }, (_, i) => fn(i + 1));

/** E06.1 压力夹具（每个覆盖至少一项载荷维度）。 */
export const E06_SCENARIOS = {
  // 大正文/工具行 + 非局部依赖：尾部 assistant 的 turn-input 在首页（非局部），
  // 中部一条 100KiB 工具结果行
  "big-tool-result": () => {
    const lines = [HEADER("e06-bigtool")];
    lines.push(msg("u1", null, { role: "user", content: "开始" }));
    let parent = "u1";
    for (let i = 1; i <= 30; i += 1) {
      lines.push(msg(`a${i}`, parent, { role: "assistant", content: `回复 ${i}` }));
      parent = `a${i}`;
      if (i === 15) {
        lines.push(msg(`r15`, parent, { role: "toolResult", content: [{ type: "text", text: `BIG-${"y".repeat(100 * 1024)}` }] }));
        parent = "r15";
      }
    }
    return Buffer.from(lines.join("\n") + "\n", "utf8");
  },
  // 多块：assistant 多 content 块（text/thinking 交替 → 多块产出）
  "multi-block": () => {
    const lines = [HEADER("e06-multiblock")];
    lines.push(msg("u1", null, { role: "user", content: "开始" }));
    let parent = "u1";
    for (let i = 1; i <= 40; i += 1) {
      lines.push(msg(`a${i}`, parent, {
        role: "assistant",
        content: [
          { type: "thinking", thinking: `思考 ${i}` },
          { type: "text", text: `回答 ${i}` },
          { type: "text", text: `补充 ${i}` },
        ],
      }));
      parent = `a${i}`;
    }
    return Buffer.from(lines.join("\n") + "\n", "utf8");
  },
  // 多 Run：40 组 user→assistant → 40 个 Run
  "multi-run": () => {
    const lines = [HEADER("e06-multirun")];
    let parent = null;
    for (let i = 1; i <= 40; i += 1) {
      lines.push(msg(`u${i}`, parent, { role: "user", content: `问题 ${i}` }));
      lines.push(msg(`a${i}`, `u${i}`, { role: "assistant", content: `回答 ${i}` }));
      parent = `a${i}`;
    }
    return Buffer.from(lines.join("\n") + "\n", "utf8");
  },
  // 单超长 Run：一个 user 后 300 条 assistant（单 Run）
  "superlong-run": () => {
    const lines = [HEADER("e06-superlong")];
    lines.push(msg("u1", null, { role: "user", content: "开始" }));
    for (let i = 1; i <= 300; i += 1) {
      lines.push(msg(`a${i}`, i === 1 ? "u1" : `a${i - 1}`, { role: "assistant", content: `长跑 ${i}` }));
    }
    return Buffer.from(lines.join("\n") + "\n", "utf8");
  },
  // 隐藏整页：60 条连续 display:false 的 custom 记录（整页被前端过滤仍可推进）
  "hidden-page": () => {
    const lines = [HEADER("e06-hidden")];
    lines.push(msg("u1", null, { role: "user", content: "开始" }));
    for (let i = 1; i <= 60; i += 1) {
      lines.push(custom(`c${i}`, i === 1 ? "u1" : `c${i - 1}`, "hana-background-result", { note: `hidden ${i}` }, { display: false, content: "" }));
    }
    return Buffer.from(lines.join("\n") + "\n", "utf8");
  },
  // 非局部依赖：工具结果在尾部页，其 assistant 在首屏（跨页依赖，翻末页时触发）
  "non-local-dependency": () => {
    const lines = [HEADER("e06-nonlocal")];
    lines.push(msg("u1", null, { role: "user", content: "开始" }));
    for (let i = 1; i <= 80; i += 1) {
      lines.push(msg(`a${i}`, i === 1 ? "u1" : `a${i - 1}`, { role: "assistant", content: `主链 ${i}` }));
    }
    lines.push(msg("r-last", "a80", { role: "toolResult", content: [{ type: "text", text: "远端结果" }] }));
    return Buffer.from(lines.join("\n") + "\n", "utf8");
  },
};

export async function runE06Experiments(ctx) {
  const { args, allRecords, cells, failures, summary, tmpRoot, counters, fx, mods, outDir } = ctx;
  const experiments = {};
  const pct = (arr, q) => (arr.length ? arr[Math.min(arr.length - 1, Math.ceil(q * arr.length) - 1)] : null);

  for (const n of args.sizes) {
    for (const ps of args.pageSizes) {
      const rows = allRecords.filter((r) => r.cell?.size === n && r.cell?.pageSize === ps);
      const first = rows.filter((r) => r.behavior === "first-screen");
      const walk = rows.filter((r) => r.behavior === "walk-forward");
      const firstWalls = first.map((r) => r.clientMs.total).sort((a, b) => a - b);
      const hotWalls = walk.filter((r) => r.seqInBehavior > 1).map((r) => r.clientMs.total).sort((a, b) => a - b);
      const jsonMs = walk.map((r) => r.clientMs.jsonParseMs);
      const mem = { heapDeltaMax: 0, externalDeltaMax: 0, arrayBuffersDeltaMax: 0 };
      let bodyKiBPerPage = null;
      if (walk.length) bodyKiBPerPage = Number((walk.reduce((s2, r) => s2 + r.bodyBytes, 0) / walk.length / 1024).toFixed(1));
      for (const r of walk) {
        mem.heapDeltaMax = Math.max(mem.heapDeltaMax, r.memory?.heapUsedDelta ?? 0);
        mem.externalDeltaMax = Math.max(mem.externalDeltaMax, r.memory?.externalDelta ?? 0);
        mem.abMax = Math.max(mem.abMax, r.memory?.arrayBuffersDelta ?? 0);
      }
      const byCombo = {};
      for (const c2 of cells.filter((c2x) => c2x.size === n && c2x.pageSize === ps)) {
        byCombo[c2.shaped ? `rtt${c2.rttMs}-bw${c2.bandwidthMbps}` : "loopback"] = {
          firstScreenWallMs: Number(c2.behaviors["first-screen"].wallMs.toFixed(1)),
          walkRequests: c2.behaviors["walk-forward"].requests,
          walkWallMs: Number(c2.behaviors["walk-forward"].wallMs.toFixed(0)),
          walkTotalKiB: Number((c2.behaviors["walk-forward"].totalBytes / 1024).toFixed(0)),
        };
      }
      experiments[`n${n}-K${ps}`] = {
        firstScreenClientMs: {
          p50: pct(firstWalls, 0.5), p95: pct(firstWalls, 0.95), max: firstWalls[firstWalls.length - 1] ?? null, samples: firstWalls.length,
        },
        hotPageClientMs: {
          p50: pct(hotWalls, 0.5), p95: pct(hotWalls, 0.95), max: hotWalls[hotWalls.length - 1] ?? null, samples: hotWalls.length,
        },
        walkJsonParseMsP50: pct(jsonMs, 0.5),
        peakSinglePageMemoryDelta: mem,
        avgBodyKiBPerPage: bodyKiBPerPage,
        byCombo,
      };
      console.log(`[e06] n=${n} K=${ps}: firstP50=${experiments[`n${n}-K${ps}`].firstScreenClientMs.p50?.toFixed(1)}ms hotP50=${experiments[`n${n}-K${ps}`].hotPageClientMs.p50?.toFixed(2)}ms hotP95=${experiments[`n${n}-K${ps}`].hotPageClientMs.p95?.toFixed(2)}ms body/K=${bodyKiBPerPage}KiB`);
    }
  }
  summary.experiments = experiments;
}

/** etag 随 limit 变化校验 + 压力场景（独立 HTTP env）。 */
export async function runE06Checks(ctx) {
  const { args, failures, summary, tmpRoot, counters, fx, mods, outDir } = ctx;
  summary.etagVariesByLimit = {};
  const stress = {};

  for (const n of args.sizes) {
    const root = fs.mkdtempSync(path.join(tmpRoot, `e06-etag-${n}-`));
    const env = fx.createBenchEnvironment({ mods, counters, n, rootDir: root, fixtureBytes: fx.buildLongRunFixtureBytes(n) });
    await fx.attachBenchApp(env, { mods, counters, engineOverrides: {} });
    const server = serve({ fetch: (req) => env.app.fetch(req), port: 0, hostname: "127.0.0.1" });
    await new Promise((r) => (server.address() ? r() : server.once("listening", r)));
    const port = server.address().port;
    const etagOf = async (limit) => {
      const res = await fetch(`http://127.0.0.1:${port}${fx.messagesUrl(env.sessionPath, { before: null, limit })}`);
      const etag = res.headers.get("etag");
      await res.text();
      return etag;
    };
    const e1 = await etagOf(50);
    const e2 = await etagOf(100);
    const varies = e1 != null && e2 != null && e1 !== e2;
    summary.etagVariesByLimit[n] = varies;
    if (!varies) failures.push(`n=${n}：同页不同 limit 的 ETag 相同（请求身份未含 limit，E06.1 违规）`);
    server.close();
    fx.teardownBenchEnvironment(env);
    fs.rmSync(root, { recursive: true, force: true });
  }
  console.log(`[e06] etagVariesByLimit: ${JSON.stringify(summary.etagVariesByLimit)}`);

  // 压力场景（每候选 K；loopback 直连，聚焦窗口/解析边界）
  for (const ps of args.pageSizes) {
    stress[`K${ps}`] = {};
    for (const name of Object.keys(E06_SCENARIOS)) {
      const root = fs.mkdtempSync(path.join(tmpRoot, `e06-${name}-K${ps}-`));
      const fixtureBytes = E06_SCENARIOS[name]();
      const env = fx.createBenchEnvironment({ mods, counters, n: 0, rootDir: root, fixtureBytes });
      await fx.attachBenchApp(env, { mods, counters, engineOverrides: {} });
      const server = serve({ fetch: (req) => env.app.fetch(req), port: 0, hostname: "127.0.0.1" });
      await new Promise((r) => (server.address() ? r() : server.once("listening", r)));
      const port = server.address().port;
      const raw = async (limit) => {
        const res = await fetch(`http://127.0.0.1:${port}${fx.messagesUrl(env.sessionPath, { before: null, limit })}`);
        const text = await res.text();
        return { status: res.status, body: JSON.parse(text) };
      };
      const cold = await raw(ps);
      const hot = await raw(ps);
      stress[`K${ps}`][name] = {
        status: cold.status,
        coldDisplay: (cold.body.messages ?? []).length,
        coldHasMore: cold.body.hasMore ?? false,
        hotDisplay: (hot.body.messages ?? []).length,
        hotHasMore: hot.body.hasMore ?? false,
      };
      if (cold.status !== 200 || hot.status !== 200) {
        failures.push(`e06 stress ${name} K=${ps}：非 200（${cold.status}/${hot.status}）`);
      }
      server.close();
      fs.rmSync(root, { recursive: true, force: true });
      console.log(`[e06] stress K=${ps} ${name}: cold=${stress[`K${ps}`][name].coldDisplay}/${stress[`K${ps}`][name].coldHasMore} hot=${stress[`K${ps}`][name].hotDisplay}/${stress[`K${ps}`][name].hotHasMore}`);
    }
  }
  summary.stress = stress;
}
