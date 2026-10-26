#!/usr/bin/env node
// R08-T01 §6.1 — 三方绑定表生成器：API_COMPAT_MATRIX business 项 × Rust 现役路由
// × renderer 实际消费者/映射。
//
//   node scripts/rust-tauri/r08_t01_api_binding.mjs           # 生成绑定表
//   node scripts/rust-tauri/r08_t01_api_binding.mjs --check   # 复算并 diff，漂移退出 1
//
// 诚实边界：
// - 映射判定调用的是真实 serviceRoute（经 TypeScript 转译后求值，工具侧只
//   追加导出，不改产品源码），不是复刻的正则副本。
// - Rust 路由来自 rust/crates/lingxi-service/src 生产源码的 .route 注册扫描；
//   行内注释已剔除，块注释用朴素扫描剔除。一次注册多方法按方法级展开。
// - 消费者引用是 desktop/src 下字符串字面量（含模板字面量静态前缀）的
//   形状匹配；它证明“引用存在”，不证明运行时调用成功。
// - preload/PlatformApi 207 项属宿主桥域，标 HOST_BRIDGE_OUT_OF_HTTP_SCOPE，
//   不在本表 HTTP 绑定范围内（见 CLIENT_BOUNDARY_PREPARATION）。
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const ts = require('typescript');
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const MATRIX = path.join(REPO, 'docs/rust-tauri/R01/API_COMPAT_MATRIX.json');
const OUT = path.join(REPO, 'docs/rust-tauri/R08/T01_API_BINDING_TABLE.json');
const CHECK = process.argv.includes('--check');

const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const rel = p => path.relative(REPO, p).replaceAll(path.sep, '/');

// ── 1. Rust 路由提取 ────────────────────────────────────────────────────────
function stripRustComments(src) {
  // 朴素块注释剔除（不处理字符串里的 /*，路由注册行不含此类字符串）。
  return src.replace(/\/\*[\s\S]*?\*\//g, m => ' '.repeat(m.length));
}
function extractRustRoutes() {
  const dir = path.join(REPO, 'rust/crates/lingxi-service/src');
  const files = [];
  const walk = d => fs.readdirSync(d, { withFileTypes: true }).forEach(e => {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith('.rs')) files.push(p);
  });
  walk(dir);
  const routes = []; // {path, methods[], file, line}
  for (const file of files) {
    const raw = fs.readFileSync(file, 'utf-8');
    const src = stripRustComments(raw);
    const lineOf = idx => raw.slice(0, idx).split('\n').length;
    const re = /\.route\(\s*"([^"]+)"/g;
    let m;
    while ((m = re.exec(src))) {
      // 行内注释剔除：该行 match 之前出现 // 的跳过（块注释已抹掉）。
      const lineStart = src.lastIndexOf('\n', m.index) + 1;
      if (src.slice(lineStart, m.index).includes('//')) continue;
      // .route( 第二参数到匹配右括号，收集方法链。
      let depth = 1, i = m.index + m[0].length;
      while (i < src.length && depth > 0) {
        if (src[i] === '(') depth++;
        else if (src[i] === ')') depth--;
        i++;
      }
      const region = src.slice(m.index, i);
      const methods = [...region.matchAll(/\b(get|post|put|patch|delete|head|options)\s*\(/g)]
        .map(x => x[1].toUpperCase());
      routes.push({ path: m[1], methods: [...new Set(methods)], file: rel(file), line: lineOf(m.index) });
    }
  }
  // 同一路径多处注册视为一处（生产库不允许；重复则保留并标记）。
  const byPath = new Map();
  for (const r of routes) {
    const k = r.path;
    if (!byPath.has(k)) byPath.set(k, { path: k, methods: new Set(), files: [] });
    const e = byPath.get(k);
    r.methods.forEach(x => e.methods.add(x));
    e.files.push(`${r.file}:${r.line}`);
  }
  return [...byPath.values()].map(e => ({ path: e.path, methods: [...e.methods].sort(), files: e.files }));
}

// ── 2. 真实 serviceRoute 求值（转译 + 迷你模块加载器） ────────────────────────
const ADAPTER_FILES = [
  'desktop/src/react/services/rust-automation.ts',
  'desktop/src/react/services/rust-bridge.ts',
  'desktop/src/react/services/rust-browser.ts',
  'desktop/src/react/services/rust-knowledge.ts',
  'desktop/src/react/services/rust-observability.ts',
  'desktop/src/react/services/rust-workbench.ts',
  'desktop/src/react/services/rust-settings.ts',
];
const HOOK_FILE = 'desktop/src/react/hooks/use-hana-fetch.ts';
const WANTED = new Set([HOOK_FILE, ...ADAPTER_FILES]);
// 良性深桩：serviceRoute 求值路径不应触达 stores/server-connection 等依赖；
// 误触达会破坏下方自检锚点而暴露，不会静默出错。
const stub = new Proxy(function stub() {}, {
  get(t, k) {
    if (k === Symbol.toPrimitive) return () => '';
    if (k === 'prototype') return {};
    return stub;
  },
  apply: () => undefined,
});
const moduleCache = new Map();
function loadTs(absPath, extraTail = '') {
  const key = absPath + (extraTail ? '+tail' : '');
  if (moduleCache.has(key)) return moduleCache.get(key).exports;
  const src = fs.readFileSync(absPath, 'utf-8');
  const out = ts.transpileModule(src, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const fn = new Function('module', 'exports', 'require', out + extraTail);
  const module = { exports: {} };
  moduleCache.set(key, module);
  fn(module, module.exports, spec => resolveSpec(absPath, spec));
  return module.exports;
}
function resolveSpec(fromFile, spec) {
  const base = path.resolve(path.dirname(fromFile), spec);
  for (const cand of [base + '.ts', base + '.tsx', base + '/index.ts']) {
    if (!fs.existsSync(cand)) continue;
    if (WANTED.has(rel(cand))) return loadTs(cand);
    break; // 存在但不在白名单 → 桩
  }
  return stub;
}
function realServiceRoute() {
  const hook = loadTs(
    path.join(REPO, HOOK_FILE),
    '\nmodule.exports.__serviceRoute = serviceRoute;\nmodule.exports.__translate = translateLegacySessionCall;',
  );
  if (typeof hook.__serviceRoute !== 'function' || typeof hook.__translate !== 'function') {
    throw new Error('use-hana-fetch.ts no longer defines serviceRoute/translateLegacySessionCall — generator must be updated');
  }
  const rust = { serverNodeKind: 'lingxi-service' };
  const route = p => hook.__serviceRoute(rust, p);
  const translate = hook.__translate;
  // 自检锚点：与 use-hana-fetch.test.ts / rust-settings.test.ts 固化的现役行为一致；
  // 适配器或加载器漂移会在此失败，而不是产出错误绑定表。
  const anchors = [
    ['/api/config', '/lingxi/v1/settings/config'],
    ['/api/health', '/lingxi/v1/settings/avatar-health'],
    ['/api/knowledge/notebooks', '/lingxi/v1/knowledge/notebooks'],
    ['/api/confirm/abc', '/api/confirm/abc'],
  ];
  for (const [input, expected] of anchors) {
    const got = route(input);
    if (got !== expected) {
      throw new Error(`serviceRoute anchor drifted: ${input} -> ${got} (expected ${expected})`);
    }
  }
  return { route, translate };
}

// ── 3. 消费者字面量清点 ─────────────────────────────────────────────────────
function extractApiLiterals() {
  const dir = path.join(REPO, 'desktop/src');
  const out = []; // {literal, file, line}
  const nativeFiles = new Set(); // 同文件出现 /lingxi/v1/ 引用 → 可能存在 native 双分支
  const walk = d => fs.readdirSync(d, { withFileTypes: true }).forEach(e => {
    const p = path.join(d, e.name);
    if (e.isDirectory()) { if (e.name !== '__tests__' && !e.name.startsWith('dist')) walk(p); return; }
    if (!/\.(ts|tsx|js|cjs|mjs)$/.test(e.name) || /\.(test|spec)\./.test(e.name)) return;
    const src = fs.readFileSync(p, 'utf-8');
    if (src.includes('/lingxi/v1/')) nativeFiles.add(rel(p));
    const re = /["'`](\/api\/[^"'`\s$]*[^"'`]*)["'`]/g;
    let m;
    while ((m = re.exec(src))) {
      const literal = m[1].split('?')[0].replace(/\/+$/, '');
      const line = src.slice(0, m.index).split('\n').length;
      out.push({ literal, file: rel(p), line });
    }
  });
  walk(dir);
  return { literals: out, nativeFiles: [...nativeFiles].sort() };
}

// ── 4. 形状匹配 ────────────────────────────────────────────────────────────
const isParam = s => /^\{.+\}$/.test(s) || /^:/.test(s) || s === '__p__';
const segs = p => p.split('?')[0].split('/').filter(Boolean);
function shapeMatch(pattern, concrete) {
  const a = segs(pattern), b = segs(concrete);
  if (a.length !== b.length) return false;
  return a.every((s, i) => isParam(s) ? true : s === b[i]);
}
const toShape = p => segs(p).map(s => isParam(s) ? '{}' : s).join('/');

// ── 逐项人工裁决注记（与 r01 OVERRIDES 同一模式：改这里，不改生成物） ────────
const ADJUDICATIONS = {
  'http:POST:/api/confirm/:confirmId': {
    note:
      'T01 裁决（2026-10-22）：旧 confirmStore 流在 Rust 侧无生产者（Rust 事件/历史投影不产生 confirmation 块，全源码零命中）；四组件仅 legacy 可达。' +
      '替代链全部已接线并有真实端点：工具批准=GET/POST /lingxi/v1/sessions/{id}/approvals(/{aid})（RustSessionReviewPanel）、' +
      'ask-user=interactions/answer（InteractiveToolsPanel）、跨会话投递确认=session-collab apply/reject、定时建议=apply_suggestion、' +
      'learn_lesson 风险=R06 一次性 token。旧路径触达时由 server-connection 硬闸显式拒绝（use-hana-fetch.test.ts 已断言）。' +
      '矩阵 newProtocolCounterpart "lingxi.v1.confirm" 为无落地证据的概念标签，本裁决将其更正为 legacy-only＋显式能力错误；判据变化待独审。',
  },
  'http:POST:/api/upload-blob': {
    note:
      'T01 裁决（2026-10-22）：唯一消费者 InputArea.tsx:1130 实际经 chatFetch（chat-client.ts 的 /api/upload-blob 原生分支映射到 ' +
      '/lingxi/v1/sessions/{id}/attachments/blob），不经 lingxiFetch——本表按 lingxiFetch/serviceRoute 模型判 UNMAPPED 属消费者扫描口径误报，' +
      '真实链路已覆盖。绑定表后续版本将按包装器区分消费者。',
  },
  'http:GET:/api/sessions/messages': {
    note:
      'T01 延后（2026-10-22）：消费者 TraceDetailOverlay 经 observabilityRequest 取全量消息；Rust canonical history ' +
      '(/sessions/{id}/history) 为游标/ETag 协议且响应形状不同，URL 翻译会产生错误解析——需消费者切换 rust-chat-history 客户端，' +
      '排入 T01 消费者适配（A01 观测域）。',
  },
};

// ── 主流程 ─────────────────────────────────────────────────────────────────
function build() {
  const matrix = JSON.parse(fs.readFileSync(MATRIX, 'utf-8'));
  const rustRoutes = extractRustRoutes();
  const routesBySegCount = new Map();
  for (const r of rustRoutes) {
    const n = segs(r.path).length;
    if (!routesBySegCount.has(n)) routesBySegCount.set(n, []);
    routesBySegCount.get(n).push(r);
  }
  const findRoute = (concrete, method) => {
    for (const r of routesBySegCount.get(segs(concrete).length) || []) {
      if (!shapeMatch(r.path, concrete)) continue;
      if (method && !r.methods.includes(method)) continue;
      return r;
    }
    return null;
  };
  const routeShapeExists = concrete =>
    (routesBySegCount.get(segs(concrete).length) || []).some(r => shapeMatch(r.path, concrete));
  const { route: serviceRoute, translate } = realServiceRoute();
  const { literals, nativeFiles } = extractApiLiterals();
  const nativeSet = new Set(nativeFiles);

  const bindings = [];
  const counts = {};
  const add = status => { counts[status] = (counts[status] || 0) + 1; };

  for (const item of [
    ...matrix.surfaces.httpRoutes,
    ...matrix.surfaces.wsEndpoints,
  ]) {
    const placeholder = item.path.replace(/\{[^}]+\}/g, '__p__').replace(/:[^/]+/g, '__p__');
    const consumers = literals
      .filter(l => shapeMatch(item.path, l.literal))
      .map(l => `${l.file}:${l.line}→${l.literal}`);
    if (item.kind === 'ws_endpoint') {
      const status = item.disposition === 'controlled_deprecation'
        ? 'WS_CONTROLLED_DEPRECATED'
        : (routeShapeExists('/lingxi/v1/ws') ? 'WS_MAPPED_ROUTE_EXISTS' : 'WS_MAPPED_ROUTE_MISSING');
      add(status);
      bindings.push({ id: item.id, kind: item.kind, path: item.path, disposition: item.disposition, status, consumers });
      continue;
    }
    const mapped = serviceRoute(placeholder);
    let mappedPath = mapped.split('?')[0].replace(/\/+$/, '');
    let isMapped = mappedPath !== placeholder;
    let mappedVia = isMapped ? 'serviceRoute' : null;
    // 兼容翻译层（lingxiFetch translateLegacySessionCall）：旧 query 寻址的
    // 会话子端口。矩阵路径不带 query，用真实调用者的代表性形状求值：
    // GET ?path=native:__p__（find 另带 &q=）；PATCH body.path；POST 写端口
    // 的 body 携 path/sessionId/sessionIds 与各端口字段（R08-C③ 修正：此前
    // POST 不带 body → 翻译恒 null → rename/pin/archive/restore/…被误判
    // DUAL_BRANCH_ONLY，绑定表与真实边界不一致）。
    if (!isMapped) {
      const method = item.method.toUpperCase();
      const representativeBody = {
        path: 'native:__p__', sessionId: '__p__', sessionIds: ['__p__'],
        title: 'x', pinned: true, maxAgeDays: 30, version: 1,
        action: 'archive', cwd: '/__cwd__', workspaceMountId: '__m__',
      };
      const translateInput = method === 'PATCH' || method === 'POST'
        ? { method, body: representativeBody }
        : { method };
      const query = method === 'GET'
        ? (placeholder.endsWith('/find') ? '?path=native%3A__p__&q=x' : '?path=native%3A__p__')
        : '';
      const translated = translate(placeholder + query, translateInput);
      if (translated && typeof translated.path === 'string') {
        mappedPath = translated.path.split('?')[0].replace(/\/+$/, '');
        isMapped = true;
        mappedVia = 'legacySessionTranslation';
      }
    }
    let entry = {
      id: item.id, kind: item.kind, method: item.method, path: item.path,
      module: item.module, disposition: item.disposition,
      consumers,
    };
    if (!isMapped) {
      // 双分支启发式（如实标注为启发式）：引用旧路径的文件若同时引用
      // /lingxi/v1/ 原生路径，通常存在 native 分支，Rust 连接下不走旧路径；
      // 全部消费者都在这类文件 → DUAL_BRANCH_ONLY；任一消费者在无原生引用
      // 文件 → DIRECT（Rust 连接下该调用必被硬闸拒绝）。
      const consumerFiles = [...new Set(consumers.map(c => c.split(':')[0] + ':' + c.split(':')[1]))];
      const direct = consumerFiles.some(f => {
        const file = f.slice(0, f.lastIndexOf(':'));
        return !nativeSet.has(file);
      });
      const status = consumers.length === 0 ? 'UNMAPPED_NOT_CONSUMED'
        : direct ? 'UNMAPPED_REFERENCED_DIRECT' : 'UNMAPPED_REFERENCED_DUAL_BRANCH_ONLY';
      add(status);
      bindings.push({ ...entry, status, mappedPath: null });
      continue;
    }
    const withMethod = findRoute(mappedPath, item.method);
    const status = withMethod ? 'MAPPED_ROUTE_EXISTS'
      : routeShapeExists(mappedPath) ? 'MAPPED_METHOD_MISSING'
      : 'MAPPED_ROUTE_MISSING';
    add(status);
    const adjudication = ADJUDICATIONS[item.id];
    bindings.push({
      ...entry, status, mappedPath,
      ...(mappedVia ? { mappedVia } : {}),
      rustRoute: withMethod ? { path: withMethod.path, methods: withMethod.methods, files: withMethod.files } : null,
      ...(adjudication || {}),
    });
  }
  for (const item of [...matrix.surfaces.preloadBridge, ...matrix.surfaces.platformApi]) {
    add('HOST_BRIDGE_OUT_OF_HTTP_SCOPE');
    bindings.push({ id: item.id, kind: item.kind, status: 'HOST_BRIDGE_OUT_OF_HTTP_SCOPE' });
  }
  return {
    schema: 'lingxi.r08.t01-api-binding.v1',
    generatedBy: 'scripts/rust-tauri/r08_t01_api_binding.mjs',
    generatedAt: new Date().toISOString(),
    inputs: {
      matrix: rel(MATRIX), matrixSha256: sha(MATRIX),
      useHanaFetch: 'desktop/src/react/hooks/use-hana-fetch.ts', useHanaFetchSha256: sha(path.join(REPO, 'desktop/src/react/hooks/use-hana-fetch.ts')),
    },
    method: '映射=真实 serviceRoute 求值（TypeScript 转译后调用，未改产品源码）；路由=rust/crates/lingxi-service/src 生产源码 .route 注册扫描（注释剔除）；消费者=desktop/src 字符串字面量形状匹配。绑定表证明引用与映射存在，不是运行时验收。',
    rustRouteCount: rustRoutes.length,
    summary: counts,
    bindings,
  };
}

const result = build();
const text = JSON.stringify(result, null, 2) + '\n';
if (CHECK) {
  if (!fs.existsSync(OUT)) { console.error('binding table missing; run without --check first'); process.exit(1); }
  const existing = fs.readFileSync(OUT, 'utf-8');
  const a = JSON.parse(existing), b = JSON.parse(text);
  a.generatedAt = b.generatedAt = undefined;
  if (JSON.stringify(a) !== JSON.stringify(b)) {
    console.error('binding table drift detected — regenerate without --check');
    process.exit(1);
  }
  console.log('binding table stable');
} else {
  fs.writeFileSync(OUT, text);
  console.log(`wrote ${rel(OUT)}: ${result.rustRouteCount} rust routes`);
  console.log(JSON.stringify(result.summary, null, 1));
}
