#!/usr/bin/env node
/**
 * benchmark-history-protocol.mjs — D07 真实传输协议基线（TASKBOOK §D07）
 *
 * 真实 HTTP：Hono app（与 D01 同一 app 构造）经 @hono/node-server 挂在 127.0.0.1
 * loopback 上；受控链路 = loopback 之上的本地 TCP 代理，做确定性延迟注入与带宽整形。
 * 不调用公网模型；隔离临时 home / 合成会话 / 独立 manifest store（复用 D01 fixture harness）。
 *
 * 三种口径分开记录，不得混算：
 *  1. honoSpanMs   —— app.fetch 服务端 span（与 D01 同仪表，counters 复用）；
 *  2. HTTP loopback —— 127.0.0.1 直连（无整形；cell 标 rtt=null,bw=null）；
 *  3. 受控链路     —— 代理整形后。均为本机确定性条件，不代表公网质量
 *     （无丢包/抖动/拥塞/真实 RTT 拓扑）。
 *
 * rtt-ms 语义：总附加往返延迟。分配方式：每请求新建 TCP 连接（Connection: close），
 * 请求方向首字节延迟 rtt/2 + 响应方向首字节延迟 rtt/2，合计每请求恰好一次附加往返，
 * 不会无意变两倍（TCP 握手在本机 loopback，不注入延迟）。
 * bandwidth-mbps：每方向独立 token bucket（桶容量 64KiB，连续补充；只整形成功后的
 * 数据字节）。可测范围：rbt 0–150ms、带宽 10–50Mbps（可加档），桶容量保证小页
 * （<64KiB）不被桶容量放大延迟，只受速率限制。
 *
 * 行为矩阵（现状记录，不实现任何 E 功能——不加 ETag/概览/页大小头）：
 *  a first-screen 新进入会话首屏（1 个普通分页请求，不强制翻完全部历史）；
 *  b revalidation 已持有同一页的重复校验（现状=客户端重新完整 GET 同页）；
 *  c walk-forward 完整向前翻页（沿 nextBefore 至终点，1k=21 页/10k=201 页）；
 *  d overview-tasks 只取总量/任务分布（现状协议无该端点：requests=0 如实记录，
 *    不虚构强制全翻；今日达成同一目标需 walk-forward 级请求数）。
 *
 * 客户端成本复现边界：Node 侧复现「读取响应 + JSON 解码 + initSession 状态应用
 * 骨架（items 浅映射替换 + hasMore/revision/nextBefore 应用）」；buildItemsFromHistory
 * 的 markdown 渲染/附件解析/assistant 块切分与 React 提交/渲染属桌面渲染链，
 * Node 侧不可复现，如实标注为缺口（uiApplyMs/renderMs = null）。
 *
 * 用法：
 *   node scripts/benchmark-history-protocol.mjs \
 *     --phase D --sizes 1000,10000 --seed 20260910 --page-sizes 50 \
 *     --rtt-ms 0,50,150 --bandwidth-mbps 10,50 \
 *     --output artifacts/history-read-directory/protocol/baseline-d
 * 未知参数报错（exit 2）；phase E/F 预留未实现（exit 2）；有校验失败 exit 1；不吞退出码。
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import http from "node:http";
import net from "node:net";
import { fileURLToPath } from "node:url";
import { serve } from "@hono/node-server";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function parseArgs(argv) {
  const args = {
    phase: null,
    sizes: [1000, 10000],
    seed: 20260910,
    pageSizes: [50],
    rttMs: [0, 50, 150],
    bandwidthMbps: [10, 50],
    output: null,
  };
  const valueArgs = new Set(["--phase", "--sizes", "--seed", "--page-sizes", "--rtt-ms", "--bandwidth-mbps", "--output"]);
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!valueArgs.has(arg)) {
      console.error(`未知参数：${arg}（支持：${[...valueArgs].join(" ")}）`);
      process.exit(2);
    }
    const value = argv[i + 1];
    if (value === undefined) {
      console.error(`参数 ${arg} 缺少值`);
      process.exit(2);
    }
    switch (arg) {
      case "--phase": args.phase = value.toUpperCase(); break;
      case "--sizes": args.sizes = value.split(",").map((x) => Number(x)); break;
      case "--seed": args.seed = Number(value); break;
      case "--page-sizes": args.pageSizes = value.split(",").map((x) => Number(x)); break;
      case "--rtt-ms": args.rttMs = value.split(",").map((x) => Number(x)); break;
      case "--bandwidth-mbps": args.bandwidthMbps = value.split(",").map((x) => Number(x)); break;
      case "--output": args.output = value; break;
    }
    i += 1;
  }
  if (!args.phase) {
    console.error("缺少 --phase（D=本轮基线；E/F 预留）");
    process.exit(2);
  }
  if (!(args.phase === "D" || args.phase === "E" || args.phase === "F")) {
    console.error(`phase ${args.phase} 尚未实现（D=真实传输基线；E=页大小候选实验；F=最终协议复测）`);
    process.exit(2);
  }
  if (!args.output) {
    console.error("缺少 --output");
    process.exit(2);
  }
  return args;
}

const args = parseArgs(process.argv);
const outDir = path.resolve(ROOT, args.output);
fs.mkdirSync(outDir, { recursive: true });

const fx = await import("./lib/history-read-fixture.mjs");
const { HistoryDirectoryCache } = await import("../server/history-read/cache.ts");
const { createHistoryReadCounters } = await import("./lib/history-read-counters.mjs");
const { installModuleWrappers } = await import("./lib/history-read-instrumentation.mjs");

const counters = createHistoryReadCounters({ label: `history-read-protocol-${args.phase}` });
counters.install();
const uninstallHooks = installModuleWrappers();
const mods = await fx.loadProductionModules();
const { runE06Experiments, runE06Checks } = await import("./lib/e06-experiments.mjs");

const failures = [];
const startedAt = new Date().toISOString();
const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-protocol-"));

// ── 受控链路代理 ──────────────────────────────────────────────
const BUCKET_BYTES = 64 * 1024;

/**
 * 单方向整形泵：首字节不早于 releasableAt（= 方向开始 + rtt/2），之后按
 * token bucket（桶容量 64KiB、连续补充、部分放行）确定性放行。
 * 预算上限：队列 >8MiB 时暂停源（本基线页体量远小于该值）。
 */
class ShapedPump {
  constructor(src, dst, firstDelayMs, bytesPerSec) {
    this.src = src;
    this.dst = dst;
    this.firstDelayMs = firstDelayMs;
    this.rate = bytesPerSec; // Infinity = 不整形
    this.capacity = BUCKET_BYTES;
    this.tokens = this.capacity;
    this.lastRefill = Date.now();
    this.releasableAt = null; // 首块数据到达时起算 firstDelayMs（避免与对向延迟重叠）
    this.queue = [];
    this.ended = false;
    this.timer = null;
    this.paused = false;
    src.on("data", (chunk) => {
      if (this.releasableAt === null) this.releasableAt = Date.now() + this.firstDelayMs;
      this.queue.push(chunk);
      const buffered = this.queue.reduce((s, c) => s + c.length, 0);
      if (buffered > 8 * 1024 * 1024 && !this.paused) {
        this.paused = true;
        src.pause();
      }
      this.schedule();
    });
    src.on("end", () => {
      this.ended = true;
      this.schedule();
    });
    src.on("error", () => this.destroy());
    dst.on("error", () => this.destroy());
  }
  refill() {
    const now = Date.now();
    if (this.rate !== Infinity) {
      this.tokens = Math.min(this.capacity, this.tokens + ((now - this.lastRefill) * this.rate) / 1000);
    }
    this.lastRefill = now;
  }
  schedule(extraMs = 0) {
    if (this.timer) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.flush();
    }, Math.max(0, extraMs));
  }
  /** 源 socket 已关闭：不再有新数据；队列排空后 end 对端（优雅关闭）。 */
  srcClose() {
    this.ended = true;
    this.schedule();
  }
  flush() {
    const now = Date.now();
    this.refill();
    let waitMs = null;
    while (this.queue.length > 0) {
      if (now < this.releasableAt) {
        waitMs = this.releasableAt - now;
        break;
      }
      const chunk = this.queue[0];
      if (this.rate !== Infinity) {
        if (this.tokens < 1) {
          waitMs = Math.ceil((1 - this.tokens) * 1000 / this.rate);
          break;
        }
        if (chunk.length > this.tokens) {
          const take = Math.floor(this.tokens);
          const head = chunk.subarray(0, take);
          this.queue[0] = chunk.subarray(take);
          this.tokens -= take;
          this.dst.write(head);
          waitMs = 1; // 令牌耗尽，下一 tick 续放
          break;
        }
        this.tokens -= chunk.length;
      }
      this.queue.shift();
      this.releasableAt = 0; // 首字节约束只作用一次
      this.dst.write(chunk);
    }
    const buffered = this.queue.reduce((s, c) => s + c.length, 0);
    if (this.paused && buffered < BUCKET_BYTES) {
      this.paused = false;
      this.src.resume();
    }
    if (waitMs !== null) this.schedule(waitMs);
    else if (this.queue.length > 0) this.schedule(1);
    else if (this.ended) this.dst.end();
  }
  destroy() {
    if (this.timer) clearTimeout(this.timer);
    this.queue = [];
  }
}

function startProxy(targetPort, rttMs, bandwidthMbps) {
  const half = rttMs / 2;
  const rate = bandwidthMbps > 0 ? (bandwidthMbps * 1_000_000) / 8 : Infinity;
  const server = net.createServer((client) => {
    const upstream = net.connect(targetPort, "127.0.0.1");
    const c2u = new ShapedPump(client, upstream, half, rate);
    const u2c = new ShapedPump(upstream, client, half, rate);
    // 优雅关闭：源socket关闭只标记"不再有新数据"，泵把队列写完再 end 对端；
    // 立即 client.end() 会截断还在整形队列中的响应（表现为客户端 hang up）。
    const markClientClosed = () => c2u.srcClose();
    const markUpstreamClosed = () => u2c.srcClose();
    client.on("end", markClientClosed);
    client.on("close", markClientClosed);
    upstream.on("end", markUpstreamClosed);
    upstream.on("close", markUpstreamClosed);
    client.on("error", () => { c2u.destroy(); u2c.destroy(); upstream.destroy(); });
    upstream.on("error", () => { c2u.destroy(); u2c.destroy(); client.destroy(); });
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

// ── HTTP 客户端（每请求新建连接；头/正文/时延分域测量） ────────────────
function requestHeadBytes(options) {
  const lines = [`${options.method} ${options.path} HTTP/1.1`];
  const headers = { host: `${options.host}:${options.port}`, connection: "close", ...options.headers };
  for (const [k, v] of Object.entries(headers)) lines.push(`${k}: ${v}`);
  return Buffer.byteLength(lines.join("\r\n") + "\r\n\r\n", "utf8");
}

function responseHeadBytes(res) {
  let n = Buffer.byteLength(`HTTP/1.1 ${res.statusCode} ${res.statusMessage ?? ""}\r\n`, "utf8");
  for (const [k, v] of Object.entries(res.headers)) {
    for (const item of Array.isArray(v) ? v : [v]) n += Buffer.byteLength(`${k}: ${item}\r\n`, "utf8");
  }
  return n + 2;
}

function httpRequest(port, urlPath) {
  return new Promise((resolve, reject) => {
    const options = { host: "127.0.0.1", port, path: urlPath, method: "GET", headers: { accept: "application/json", connection: "close" } };
    const t0 = process.hrtime.bigint();
    const req = http.request(options, (res) => {
      const tHeaders = process.hrtime.bigint();
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("error", reject);
      res.on("end", () => {
        const tEnd = process.hrtime.bigint();
        const body = Buffer.concat(chunks);
        const tp = process.hrtime.bigint();
        let data = null;
        try { data = JSON.parse(body.toString("utf8")); } catch { /* 非 JSON（错误页）如实记录 */ }
        const jsonMs = Number(process.hrtime.bigint() - tp) / 1e6;
        const ms = (a, b) => Number(b - a) / 1e6;
        resolve({
          status: res.statusCode,
          requestHeaderBytes: requestHeadBytes(options),
          responseHeaderBytes: responseHeadBytes(res),
          bodyBytes: body.length,
          clientMs: {
            sendToHeaders: ms(t0, tHeaders),
            headersToBodyEnd: ms(tHeaders, tEnd),
            total: ms(t0, tEnd),
            jsonParseMs: jsonMs,
          },
          data,
        });
      });
    });
    req.on("error", reject);
    req.end();
  });
}

/** 客户端 merge/build 的 Node 可复现骨架（缺口见文件头注释） */
function clientApplySkeleton(data) {
  const t0 = process.hrtime.bigint();
  const messages = Array.isArray(data?.messages) ? data.messages : [];
  // initSession 的状态应用骨架：items 浅映射（id/role）替换 + 游标/修订点应用
  const items = messages.map((m) => ({ id: m?.id, role: m?.role ?? null }));
  const applied = {
    itemCount: items.length,
    hasMore: data?.hasMore ?? false,
    revision: typeof data?.revision === "string" ? data.revision : null,
    nextBefore: typeof data?.nextBefore === "string" ? data.nextBefore : data?.nextBefore === null ? null : undefined,
  };
  return { applied, applyMs: Number(process.hrtime.bigint() - t0) / 1e6 };
}

// ── 服务端挂载 ────────────────────────────────────────────────
async function startServerForSize(n, pageSize) {
  const envRoot = fs.mkdtempSync(path.join(tmpRoot, `http-${n}-`));
  const env = fx.createBenchEnvironment({ mods, counters, n, rootDir: envRoot, fixtureBytes: fx.buildLongRunFixtureBytes(n) });
  env.envRoot = envRoot;
  env.historyCache = new HistoryDirectoryCache();
  await fx.attachBenchApp(env, { mods, counters, engineOverrides: { historyReadCache: env.historyCache } });

  let last = null; // 最近一次请求的服务端记录（客户端串行，无并发归因歧义）
  const handler = async (req) => {
    const t0 = process.hrtime.bigint();
    const h = counters.beginRequest({
      fixtureId: `longrun-n${n}-seed${args.seed}`,
      phase: args.phase,
      requestKind: "http-loopback",
      limit: pageSize,
      sessionPath: env.sessionPath,
    });
    try {
      const res = await h.run(() => env.app.fetch(req));
      const spanMs = Number(process.hrtime.bigint() - t0) / 1e6;
      const rec = counters.endRequest(h);
      const g = rec.gauges;
      last = {
        honoSpanMs: spanMs,
        fullFileReadCalls: g.fullFileReadCalls,
        jsonlParseCount: g.jsonlParseCount,
        fullHistoryProjectionCount: g.fullHistoryProjectionCount,
        branchOpenCalls: g.branchOpenCalls,
        getBranchCalls: g.getBranchCalls,
        fallbackReason: g.openThrows > 0 || g.getBranchThrows > 0 ? "branch_entry_threw" : null,
      };
      return res;
    } catch (error) {
      counters.endRequest(h);
      last = { honoSpanMs: Number(process.hrtime.bigint() - t0) / 1e6, fatal: String(error?.message ?? error) };
      throw error;
    }
  };
  const server = serve({ fetch: handler, port: 0, hostname: "127.0.0.1" });
  await new Promise((resolve) => {
    if (server.address()) resolve();
    else server.once("listening", resolve);
  });
  const port = server.address().port;
  return { env, server, port, takeLast: () => { const v = last; last = null; return v; } };
}

// ── 行为驱动 ──────────────────────────────────────────────────
async function main() {
  const allRecords = [];
  const cells = [];
  let recSeq = 0;

  const summary = {
    task: args.phase === "E"
      ? "E06 页大小候选实验（K=50/100/150/200；同 seed/载荷/外部状态；含压力场景与 etag 随 limit 变化校验）"
      : "D07 真实传输协议基线（不实现 E 功能；仅记录现状）",
    phase: args.phase,
    generatedAt: startedAt,
    gitHead: "1d42b7405c76292f617291e3a01cd2f3ef5efd04",
    gitBranch: "fix/pending-sep10",
    seed: args.seed,
    cli: process.argv.slice(2).join(" "),
    failures,
  };

  for (const n of args.sizes) {
    for (const pageSize of args.pageSizes) {
      const { env, server, port, takeLast } = await startServerForSize(n, pageSize);
      const urlPath = fx.messagesUrl(env.sessionPath, { before: null, limit: pageSize });
      const displayCount = n + 1;
      const expectedPages = Math.ceil(displayCount / pageSize);
      const maxPages = expectedPages + 2; // 死循环保护（与 A05 驱动器同口径）

      const combos = [
        { label: "loopback", rttMs: null, bandwidthMbps: null, shaped: false },
        ...args.rttMs.flatMap((rtt) => args.bandwidthMbps.map((bw) => ({ label: `rtt${rtt}-bw${bw}`, rttMs: rtt, bandwidthMbps: bw, shaped: true }))),
      ];

      for (const combo of combos) {
        let proxy = null;
        let targetPort = port;
        if (combo.shaped) {
          proxy = await startProxy(port, combo.rttMs, combo.bandwidthMbps);
          targetPort = proxy.address().port;
        }
        const cell = {
          size: n,
          pageSize,
          rttMs: combo.rttMs,
          bandwidthMbps: combo.bandwidthMbps,
          shaped: combo.shaped,
          behaviors: {},
        };
        const cacheBefore = env.historyCache.stats();
        const cellT0 = process.hrtime.bigint();

        const doReq = async (behavior, seq, p) => {
          const statsBefore = env.historyCache.stats();
          const t0 = process.hrtime.bigint();
          const res = await httpRequest(targetPort, p);
          const wallMs = Number(process.hrtime.bigint() - t0) / 1e6;
          const server = takeLast() ?? {};
          const clientApply = clientApplySkeleton(res.data);
          recSeq += 1;
          allRecords.push({
            seq: recSeq,
            cell: { size: n, pageSize, rttMs: combo.rttMs, bandwidthMbps: combo.bandwidthMbps, shaped: combo.shaped },
            behavior,
            seqInBehavior: seq,
            urlPath: p,
            status: res.status,
            conditionalStatus: res.status === 304 ? "304-not-modified" : res.status,
            clientReuse: res.status === 304,
            clientFallbackReason: null,
            preflightRequests: 0,
            requestHeaderBytes: res.requestHeaderBytes,
            responseHeaderBytes: res.responseHeaderBytes,
            bodyBytes: res.bodyBytes,
            clientMs: { ...res.clientMs, stateApplyMs: clientApply.applyMs, uiApplyMs: null, renderMs: null },
            server,
            cacheDelta: {
              builds: env.historyCache.stats().builds - statsBefore.builds,
              hits: env.historyCache.stats().hits - statsBefore.hits,
              incrementalUpdates: env.historyCache.stats().incrementalUpdates - statsBefore.incrementalUpdates,
            },
            wallMs,
            gaps: "buildItemsFromHistory 完整链（markdown/附件/块切分）与 React 提交/渲染为桌面渲染链，Node 侧不可复现（uiApplyMs/renderMs=null）",
          });
          return res;
        };

        try {
          // a. first-screen
          {
            const t0 = process.hrtime.bigint();
            const res = await doReq("first-screen", 1, urlPath);
            const wallMs = Number(process.hrtime.bigint() - t0) / 1e6;
            if (res.status !== 200) failures.push(`n=${n} ${combo.label} first-screen：status=${res.status}`);
            cell.behaviors["first-screen"] = { requests: 1, wallMs, bodyBytes: res.bodyBytes, totalBytes: res.requestHeaderBytes + res.responseHeaderBytes + res.bodyBytes };
          }
          // b. revalidation（同一页重复完整 GET）
          {
            const t0 = process.hrtime.bigint();
            const res = await doReq("revalidation", 1, urlPath);
            const wallMs = Number(process.hrtime.bigint() - t0) / 1e6;
            cell.behaviors["revalidation"] = { requests: 1, wallMs, bodyBytes: res.bodyBytes, totalBytes: res.requestHeaderBytes + res.responseHeaderBytes + res.bodyBytes };
          }
          // c. walk-forward（沿 nextBefore 至终点；独立计数，含首页）
          {
            const t0 = process.hrtime.bigint();
            let pages = 0;
            let bytes = 0;
            let cursor = null;
            let nextBefore = null;
            do {
              const p = fx.messagesUrl(env.sessionPath, { before: cursor, limit: pageSize });
              const res = await doReq("walk-forward", pages + 1, p);
              if (res.status !== 200) { failures.push(`n=${n} ${combo.label} walk 第 ${pages + 1} 页：status=${res.status}`); break; }
              pages += 1;
              bytes += res.requestHeaderBytes + res.responseHeaderBytes + res.bodyBytes;
              nextBefore = typeof res.data?.nextBefore === "string" ? res.data.nextBefore : null;
              cursor = nextBefore;
              if (pages > maxPages) { failures.push(`n=${n} ${combo.label} walk 超过 maxPages=${maxPages}（nextBefore 未终止）`); break; }
            } while (cursor !== null);
            const wallMs = Number(process.hrtime.bigint() - t0) / 1e6;
            if (pages !== expectedPages) failures.push(`n=${n} ${combo.label} walk 页数 ${pages} ≠ 期望 ${expectedPages}`);
            cell.behaviors["walk-forward"] = { requests: pages, wallMs, totalBytes: bytes, endNextBefore: nextBefore };
          }
          // d. overview-tasks（现状无端点：如实记录，不虚构强制全翻）
          {
            cell.behaviors["overview-tasks"] = {
              supported: false,
              requests: 0,
              note: "当前协议无总量/任务分布端点；现状达成同一用户目标只能整会话翻页探底（请求数≈walk-forward.requests）或不可得；本基线不虚构强制全翻，亦未实现任何 E 概览功能",
            };
          }
          cell.wallMs = Number(process.hrtime.bigint() - cellT0) / 1e6;
          cells.push(cell);
          console.log(`[proto] n=${n} pageSize=${pageSize} ${combo.label}: first=${cell.behaviors["first-screen"].wallMs.toFixed(1)}ms reval=${cell.behaviors["revalidation"].wallMs.toFixed(1)}ms walk=${cell.behaviors["walk-forward"].requests}页/${cell.behaviors["walk-forward"].wallMs.toFixed(0)}ms/${(cell.behaviors["walk-forward"].totalBytes / 1024).toFixed(0)}KiB`);
        } finally {
          if (proxy) await new Promise((r) => proxy.close(() => r()));
        }
      }
      server.close();
      fx.teardownBenchEnvironment(env);
      fs.rmSync(env.envRoot, { recursive: true, force: true });
    }
  }
  // ── E06.1：逐候选聚合 + 压力场景 + etag 随 limit 变化校验 ──
  if (args.phase === "E" || args.phase === "F") {
    await runE06Experiments({ args, allRecords, cells, failures, summary, tmpRoot, counters, fx, mods, outDir });
    await runE06Checks({ args, failures, summary, tmpRoot, counters, fx, mods, outDir });
  }

  // ── F02：决定性 304 测试（同页重复校验 20 次：全 304、bodyBytes=0、客户端零解码、零重建） ──
  if (args.phase === "F") {
    summary.decisive304 = {};
    for (const n of args.sizes) {
      const envRoot = fs.mkdtempSync(path.join(tmpRoot, `decisive-${n}-`));
      const env = fx.createBenchEnvironment({ mods, counters, n, rootDir: envRoot, fixtureBytes: fx.buildLongRunFixtureBytes(n) });
      env.envRoot = envRoot;
      env.historyCache = new HistoryDirectoryCache();
      await fx.attachBenchApp(env, { mods, counters, engineOverrides: { historyReadCache: env.historyCache } });
      const server = serve({ fetch: (req) => env.app.fetch(req), port: 0, hostname: "127.0.0.1" });
      await new Promise((r) => (server.address() ? r(undefined) : server.once("listening", () => r(undefined))));
      const port = server.address().port;
      try {
        const url50 = fx.messagesUrl(env.sessionPath, { before: null, limit: 50 });
        const r1 = await fetch(`http://127.0.0.1:${port}/api/sessions/messages?path=${encodeURIComponent(env.sessionPath)}&limit=50`);
        await r1.text();
        const etag = r1.headers.get("etag");
        const buildsBefore = env.historyCache.stats().builds;
        let all304 = true;
        let bodyBytesTotal = 0;
        for (let i = 1; i <= 20; i += 1) {
          const res = await fetch(`http://127.0.0.1:${port}/api/sessions/messages?path=${encodeURIComponent(env.sessionPath)}&limit=50`, {
            headers: { "if-none-match": etag, "cache-control": "no-store" },
          });
          const text = res.status === 304 ? "" : await res.text();
          if (res.status !== 304) all304 = false;
          if (text.length > 0) all304 = false;
          bodyBytesTotal += text.length;
        }
        const buildsDelta = env.historyCache.stats().builds - buildsBefore;
        summary.decisive304[`n${n}`] = {
          validations: 20,
          all304,
          bodyBytesTotal,
          clientJsonDecodeAttempts: 0, // 304 无正文：客户端无解码（结构保证）
          rebuilds: buildsDelta,
        };
        if (!all304) failures.push(`n=${n}：决定性 304 测试存在非 304 响应`);
        if (buildsDelta !== 0) failures.push(`n=${n}：决定性 304 期间发生目录重建（builds+${buildsDelta}）`);
        console.log(`[f02] n=${n}: 决定性 304 20/20=${all304} bodyBytesTotal=${bodyBytesTotal} rebuilds=${buildsDelta}`);
      } finally {
        server.close();
        fx.teardownBenchEnvironment(env);
        fs.rmSync(env.envRoot, { recursive: true, force: true });
      }
    }
  }


  fs.writeFileSync(path.join(outDir, `requests-${args.phase.toLowerCase()}.jsonl`), allRecords.map((r) => JSON.stringify(r)).join("\n") + "\n", "utf8");
  fs.writeFileSync(path.join(outDir, `summary-${args.phase.toLowerCase()}.json`), `${JSON.stringify(summary, null, 2)}\n`, "utf8");

  // summary-d.md
  const md = [
    "# phase D 协议基线（D07）",
    "",
    `- 生成：${startedAt}｜seed ${args.seed}｜页大小 ${args.pageSizes.join("/")}｜rtt ${args.rttMs.join("/")}ms｜带宽 ${args.bandwidthMbps.join("/")}Mbps`,
    "- 口径：三域分列（Hono 内存 span / HTTP loopback / 受控链路=本地代理确定性整形）；不代表公网质量。rtt 分配=每请求双向首字节各 rtt/2（合计一次往返）；带宽=每方向 64KiB 桶 token bucket。",
    "- 现状记录：无 304/无概览/无页大小头（不实现任何 E 功能）。",
    "",
    "## 行为基线（每格：请求数 / wall ms / 总字节 KiB）",
    "",
    "| 规模 | 链路 | a 首屏 | b 重复校验 | c 完整翻页 | d 总量/任务 |",
    "|---|---|---|---|---|---|",
  ];
  for (const cell of cells) {
    const a = cell.behaviors["first-screen"];
    const b = cell.behaviors["revalidation"];
    const c = cell.behaviors["walk-forward"];
    const d = cell.behaviors["overview-tasks"];
    md.push(`| ${cell.size} | ${cell.shaped ? `rtt=${cell.rttMs}/bw=${cell.bandwidthMbps}` : "loopback"} | 1 / ${a.wallMs.toFixed(1)} / ${(a.totalBytes / 1024).toFixed(1)} | 1 / ${b.wallMs.toFixed(1)} / ${(b.totalBytes / 1024).toFixed(1)} | ${c.requests} / ${c.wallMs.toFixed(0)} / ${(c.totalBytes / 1024).toFixed(0)} | 0（无端点，见 note） |`);
  }
  md.push(
    "",
    "说明：a/b 单请求；b 即「重复校验现状」=重新完整 GET 同页（正文字节与 a 相同量级）；c 沿 nextBefore 至终点（1k=21 页/10k=201 页）；d 现状无端点（requests=0 如实记录，不虚构强制全翻；若今日强行达成需 c 级请求数）。",
    "",
    failures.length ? `**失败/异常项（${failures.length}）**：` : "**失败/异常项**：无",
    ...failures.map((f) => `- ${f}`),
    "",
  );

  fs.writeFileSync(path.join(outDir, `summary-${args.phase.toLowerCase()}.md`), md.join("\n"), "utf8");

  console.log(`D07 协议基线完成：${failures.length === 0 ? "全部校验通过" : `存在 ${failures.length} 项失败`}；输出 ${path.relative(ROOT, outDir)}`);
  if (failures.length > 0) process.exitCode = 1;
}

try {
  await main();
} finally {
  uninstallHooks();
  try { fs.rmSync(tmpRoot, { recursive: true, force: true }); } catch { /* 临时目录清理失败不影响退出码 */ }
}
