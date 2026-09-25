#!/usr/bin/env node
// R01-T02 step 3 — extracts the incumbent API surface from REAL SOURCE CODE
// (never hand-copied from docs) and builds API_COMPAT_MATRIX.json.
//
// Sources scanned:
//   HTTP routes   server/index.ts, server/composition/{open-root,full-root}.ts
//                 (mount table) + server/routes/*.ts (route.METHOD("path"))
//   WS endpoints  upgradeWebSocket call sites + the raw /internal/browser
//                 upgrade handler in server/index.ts
//   preload       desktop/preload.cjs (window.hana members + IPC channels)
//   PlatformApi   desktop/src/react/types.ts (member list)
//
// Usage:
//   node scripts/rust-tauri/r01-t02-extract-api-surface.mjs           # write matrix
//   node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --check   # diff, exit 1 on drift
//   node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --inventory-only  # print raw inventory
//
// The matrix maps every item to exactly one category
// (business_compat | native_host) and one disposition
// (retain | controlled_deprecation | native_host_mapping) plus its new
// protocol counterpart where one exists. Dispositions that cannot be
// derived by rule come from the explicit OVERRIDES table below — edit the
// table, never the generated JSON.

import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const MATRIX_PATH = path.join(REPO, "docs/rust-tauri/R01/API_COMPAT_MATRIX.json");
const args = process.argv.slice(2);
const CHECK = args.includes("--check");
const INVENTORY_ONLY = args.includes("--inventory-only");

// ── Explicit per-item overrides (rule-derivable defaults live in classify()) ──
// key: "kind:METHOD:path" for routes/ws, "preload:<member>" for bridge entries.
const OVERRIDES = {
  "ws:GET:/internal/browser": {
    disposition: "controlled_deprecation",
    newProtocolCounterpart: "lingxi.v1.browser-engine control channel (R04/R09 原型; W03)",
    notes:
      "Raw ws.WebSocketServer 旁路（WsTransport 依赖裸 ws API）。新协议下由浏览器引擎控制通道承载；" +
      "退役前保留，退役需 R08 旧客户端下线证据。",
  },
  "preload:getServerPort": {
    category: "business_compat",
    disposition: "retain",
    newProtocolCounterpart: "lingxi.wire bootstrap: server-info discovery + handshake (PROTOCOL_SPEC §7)",
    notes: "连接引导信息，迁移期经宿主桥提供，最终由 service 发现机制承载。",
  },
  "preload:getServerToken": {
    category: "business_compat",
    disposition: "retain",
    newProtocolCounterpart: "lingxi.wire bootstrap: loopback token handoff (R02 认证层)",
    notes: "本机最高权限凭据分发；新协议下必须继续走 owner-only 通道，不进网页可枚举面。",
  },
};

// Modules whose routes map to a controlled deprecation wholesale (none at
// R01; placeholder for the rule, kept explicit so a future stage edits the
// script, not the JSON).
const MODULE_OVERRIDES = {};

// ── Extraction ─────────────────────────────────────────────────────────────

function sha256File(p) {
  return createHash("sha256").update(fs.readFileSync(p)).digest("hex");
}

function read(rel) {
  return fs.readFileSync(path.join(REPO, rel), "utf-8");
}

/** Mount table: app.route("/api", createXRoute(...)) across composition roots + index. */
function extractMounts() {
  const mounts = []; // {mountPath, factory, file}
  for (const rel of [
    "server/index.ts",
    "server/composition/open-root.ts",
    "server/composition/full-root.ts",
  ]) {
    const src = read(rel);
    const re = /app\.route\(\s*"([^"]*)",\s*(\w+)\s*\(/g;
    let m;
    while ((m = re.exec(src))) {
      mounts.push({ mountPath: m[1], factory: m[2], file: rel });
    }
    // import map: factory -> route module file
  }
  return mounts;
}

/** factory name -> route module file (from imports in the composition files). */
function extractFactoryModules() {
  const map = {};
  for (const rel of [
    "server/index.ts",
    "server/composition/open-root.ts",
    "server/composition/full-root.ts",
  ]) {
    const src = read(rel);
    const re = /import\s*\{([^}]*)\}\s*from\s*"\.\.\/routes\/([\w-]+)\.ts"/g;
    let m;
    while ((m = re.exec(src))) {
      const names = m[1].split(",").map((s) => s.trim().replace(/^type\s+/, ""));
      for (const n of names) {
        if (n) map[n] = `server/routes/${m[2]}.ts`;
      }
    }
  }
  return map;
}

/** route.METHOD("path") declarations inside every server/routes/*.ts + index.ts. */
function extractHttpRoutes() {
  const routes = [];
  const files = fs
    .readdirSync(path.join(REPO, "server/routes"))
    .filter((f) => f.endsWith(".ts"))
    .map((f) => `server/routes/${f}`);
  files.push("server/index.ts");
  for (const rel of files) {
    const src = read(rel);
    const re = /\b(?:app|route|router|restRoute|wsRoute)\.(get|post|put|patch|delete)\(\s*"([^"]+)"/g;
    let m;
    while ((m = re.exec(src))) {
      const line = src.slice(0, m.index).split("\n").length;
      // A route whose handler goes through upgradeWebSocket is a WS endpoint.
      const tail = src.slice(m.index, m.index + 300);
      const isUpgrade = tail.includes("upgradeWebSocket");
      routes.push({
        method: m[1].toUpperCase(),
        path: m[2],
        file: rel,
        line,
        upgrade: isUpgrade,
      });
    }
  }
  return routes;
}

/** window.hana members + IPC channels from desktop/preload.cjs. */
function extractPreload() {
  const src = read("desktop/preload.cjs");
  const start = src.indexOf('contextBridge.exposeInMainWorld("hana", {');
  if (start < 0) throw new Error("preload: exposeInMainWorld block not found");
  // balance braces from the opening '{' of the hana object
  const open = src.indexOf("{", start);
  let depth = 0;
  let end = -1;
  for (let i = open; i < src.length; i++) {
    const ch = src[i];
    if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const block = src.slice(open, end + 1);
  const members = [];
  // depth-1 member keys: "name:" at exactly one level of nesting
  let d = 0;
  let lineStart = true;
  let current = "";
  for (const ch of block) {
    if (ch === "\n") {
      if (d === 1) {
        const m = current.match(/^\s*(\w+)\s*:/);
        if (m) members.push(m[1]);
      }
      current = "";
      continue;
    }
    if (ch === "{") d++;
    if (ch === "}") d--;
    current += ch;
  }
  const channels = new Set();
  for (const m of src.matchAll(/ipcRenderer\.(invoke|on|send)\(\s*"([^"]+)"/g)) {
    channels.add(`${m[1]}:${m[2]}`);
  }
  return { members, channels: [...channels].sort() };
}

/** PlatformApi member names from desktop/src/react/types.ts. */
function extractPlatformApi() {
  const src = read("desktop/src/react/types.ts");
  const start = src.indexOf("export interface PlatformApi");
  if (start < 0) throw new Error("PlatformApi interface not found");
  const open = src.indexOf("{", start);
  let depth = 0;
  let end = -1;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const body = src.slice(open + 1, end);
  const members = [];
  for (const line of body.split("\n")) {
    // members are method signatures: `name?(args): Ret` or `name(args): Ret`
    const m = line.match(/^\s{2}(\w+)(\?)?\s*\(/);
    if (m) members.push(m[1]);
  }
  return members;
}

// ── Classification ─────────────────────────────────────────────────────────

function moduleOfRoute(route, mounts, factoryModules) {
  if (route.file === "server/index.ts") {
    return { module: "(server/index.ts direct)", mountPrefix: "" };
  }
  // find the factory whose module file is this route file
  for (const [factory, file] of Object.entries(factoryModules)) {
    if (file === route.file) {
      const mount = mounts.find((mt) => mt.factory === factory);
      const modName = path.basename(route.file, ".ts");
      return mount
        ? { module: `${modName} (mounted ${mount.mountPath || "/"})`, mountPrefix: mount.mountPath }
        : { module: `${modName} (unmounted?)`, mountPrefix: "" };
    }
  }
  return { module: path.basename(route.file, ".ts"), mountPrefix: "" };
}

function classifyHttp(route, moduleName, mountPrefix) {
  const fullPath = joinPaths(mountPrefix, route.path);
  const key = `http:${route.method}:${fullPath}`;
  const modOverride = MODULE_OVERRIDES[moduleName];
  const base = {
    id: key,
    kind: "http_route",
    method: route.method,
    path: fullPath,
    source: `${route.file}:${route.line}`,
    module: moduleName,
    category: "business_compat",
    disposition: "retain",
    newProtocolCounterpart: null,
    notes: "",
  };
  const mod = moduleName.split(" ")[0];
  base.newProtocolCounterpart = `lingxi.v1.${mod.replace(/-/g, "_")} ${base.method} ${fullPath}`;
  if (modOverride) Object.assign(base, modOverride);
  if (OVERRIDES[key]) Object.assign(base, OVERRIDES[key]);
  return base;
}

function joinPaths(prefix, p) {
  if (!prefix) return p;
  return `${prefix.replace(/\/$/, "")}${p.startsWith("/") ? p : `/${p}`}`;
}

function classifyWs(entry) {
  const key = `ws:GET:${entry.path}`;
  const base = {
    id: key,
    kind: "ws_endpoint",
    path: entry.path,
    source: entry.source,
    module: entry.module,
    category: "business_compat",
    disposition: "retain",
    newProtocolCounterpart: `lingxi.v1 event stream (WS 承载 EventEnvelope, PROTOCOL_SPEC §5)`,
    notes: "",
  };
  if (OVERRIDES[key]) Object.assign(base, OVERRIDES[key]);
  return base;
}

function classifyPreload(member) {
  const key = `preload:${member}`;
  const base = {
    id: key,
    kind: "preload_bridge",
    member,
    category: "native_host",
    disposition: "native_host_mapping",
    newProtocolCounterpart: "tauri-host command/event (R09 宿主映射)",
    notes: "",
  };
  if (OVERRIDES[key]) Object.assign(base, OVERRIDES[key]);
  return base;
}

// ── Build ──────────────────────────────────────────────────────────────────

const mounts = extractMounts();
const factoryModules = extractFactoryModules();
const httpRoutes = extractHttpRoutes();
const preload = extractPreload();
const platformApi = extractPlatformApi();

const httpEntries = httpRoutes
  .filter((r) => !r.upgrade)
  .map((r) => {
    const { module: mod, mountPrefix } = moduleOfRoute(r, mounts, factoryModules);
    return classifyHttp(r, mod, mountPrefix);
  });

const wsEntries = [
  ...httpRoutes
    .filter((r) => r.upgrade)
    .map((r) => {
      const { module: mod, mountPrefix } = moduleOfRoute(r, mounts, factoryModules);
      return {
        path: joinPaths(mountPrefix, r.path),
        source: `${r.file}:${r.line}`,
        module: mod,
      };
    }),
  {
    path: "/internal/browser",
    source: "server/index.ts:1173 (raw ws.WebSocketServer upgrade handler)",
    module: "internal-browser",
  },
].map(classifyWs);

const preloadEntries = preload.members.map(classifyPreload);

const platformApiEntries = platformApi.map((member) => ({
  id: `platform_api:${member}`,
  kind: "platform_api_type",
  member,
  source: "desktop/src/react/types.ts (PlatformApi)",
  category: "native_host",
  disposition: "native_host_mapping",
  newProtocolCounterpart: "tauri-host command/event (R09 宿主映射)",
  notes: "window.hana 的 TS 类型镜像；与 preload_bridge 逐项对应关系由 R09 核对。",
}));

const all = [...httpEntries, ...wsEntries, ...preloadEntries, ...platformApiEntries];
const summary = { total: all.length, byKind: {}, byCategory: {}, byDisposition: {} };
for (const e of all) {
  summary.byKind[e.kind] = (summary.byKind[e.kind] ?? 0) + 1;
  summary.byCategory[e.category] = (summary.byCategory[e.category] ?? 0) + 1;
  summary.byDisposition[e.disposition] = (summary.byDisposition[e.disposition] ?? 0) + 1;
}

const sourceFiles = [
  "server/index.ts",
  "server/composition/open-root.ts",
  "server/composition/full-root.ts",
  "desktop/preload.cjs",
  "desktop/src/react/types.ts",
  ...fs
    .readdirSync(path.join(REPO, "server/routes"))
    .filter((f) => f.endsWith(".ts"))
    .map((f) => `server/routes/${f}`),
];
const sourceDigests = Object.fromEntries(
  sourceFiles.sort().map((f) => [f, sha256File(path.join(REPO, f))]),
);

const surfaces = {
  httpRoutes: httpEntries,
  wsEndpoints: wsEntries,
  preloadBridge: preloadEntries,
  platformApi: platformApiEntries,
};

const inventory = {
  mounts,
  factoryModules,
  preloadIpcChannels: preload.channels,
};

// Content-derived stamp (fix-headsha-r1): the matrix must NOT embed moving
// coordinates such as `git rev-parse HEAD` — a committed artifact containing
// the generating HEAD makes "--check ⇒ no diff" unsatisfiable on every later
// commit. contentSha is a pure function of the extracted content, so
// regeneration is byte-identical on any commit whose scanned sources are
// unchanged, and --check compares the full file with no field exemptions.
const contentSha = createHash("sha256")
  .update(JSON.stringify({ fullInventory: inventory, surfaces, summary, sourceDigests }))
  .digest("hex");

if (INVENTORY_ONLY) {
  console.log(JSON.stringify({ inventory, httpEntries, wsEntries, preloadEntries, platformApiEntries }, null, 2));
  process.exit(0);
}

const matrix = {
  specVersion: "1.0",
  task: "R01-T02",
  generatedBy: "scripts/rust-tauri/r01-t02-extract-api-surface.mjs (机械提取,禁止手改条目)",
  generatedFrom: {
    contentSha,
    note:
      "contentSha=sha256(JSON.stringify({fullInventory,surfaces,summary,sourceDigests}))," +
      "内容派生戳,不嵌入 git HEAD 等移动坐标;--check 在当前工作区重跑并全文逐字节 diff(无字段豁免)。",
    sourceDigests,
  },
  policy: {
    categories: {
      business_compat: "业务兼容映射:旧 HTTP/WS 业务能力,迁移期保留并映射到新协议对应物。",
      native_host: "原生宿主映射:窗口/对话框/通知/文件系统等宿主能力,与新业务协议分离,R09 以 Tauri command/event 承载。",
    },
    dispositions: {
      retain: "保留:迁移期旧表面继续可用,语义映射到新协议。",
      controlled_deprecation: "受控弃用:有明确替代物,退役需对应阶段证据(R08 旧客户端下线)。",
      native_host_mapping: "原生宿主映射:不进 lingxi.wire 业务协议,由 Tauri 宿主能力承接。",
    },
    versionAnchors: {
      legacy: "shared/contract-versions.json PRELOAD_API_VERSION=1 / SERVER_PROTOCOL_VERSION=1(冻结,不回退不升级)",
      target: "lingxi.wire v1(PROTOCOL_SPEC.md §2;与 legacy 轴相互独立)",
    },
  },
  inventory: {
    mountTableSize: mounts.length,
    preloadIpcChannels: preload.channels,
  },
  surfaces,
  summary,
};

const out = JSON.stringify(matrix, null, 2) + "\n";

if (CHECK) {
  const disk = fs.existsSync(MATRIX_PATH) ? fs.readFileSync(MATRIX_PATH, "utf-8") : "";
  if (disk === out) {
    console.log(`OK: API_COMPAT_MATRIX.json matches regeneration (${all.length} entries)`);
    process.exit(0);
  }
  console.error("DRIFT: API_COMPAT_MATRIX.json differs from regeneration");
  const a = disk.split("\n"), b = out.split("\n");
  let shown = 0;
  for (let i = 0; i < Math.max(a.length, b.length) && shown < 10; i++) {
    if (a[i] !== b[i]) {
      console.error(`  line ${i + 1}:\n    disk: ${a[i] ?? "<eof>"}\n    gen:  ${b[i] ?? "<eof>"}`);
      shown++;
    }
  }
  console.error("regenerate: node scripts/rust-tauri/r01-t02-extract-api-surface.mjs");
  process.exit(1);
}

fs.writeFileSync(MATRIX_PATH, out);
console.log(
  `wrote ${path.relative(REPO, MATRIX_PATH)}: ${all.length} entries ` +
    `(${summary.byKind.http_route ?? 0} http, ${summary.byKind.ws_endpoint ?? 0} ws, ` +
    `${summary.byKind.preload_bridge ?? 0} preload, ${summary.byKind.platform_api_type ?? 0} platformApi)`,
);
