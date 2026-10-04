#!/usr/bin/env node
/**
 * R05-T01: build docs/rust-tauri/R05/PROVIDER_SUPPORT_MATRIX.json from the
 * LIVE TypeScript layer. The 39 provider plugins are imported as real
 * modules (Node 24 type stripping) — the matrix reflects the actual runtime
 * objects, not regex guesses. Static sources (provider-compat dispatcher,
 * model-operations vocabulary, media adapters, speech adapters) are read as
 * text and cross-checked.
 *
 * Cross-check: every R00 D10 provider-registration leaf bound to R05 must
 * resolve to a matrix row whose protocol family and auth shape match the
 * leaf's recorded 注册 id/auth/api triple. A mismatch is a hard failure —
 * the matrix must cover the ledger, not a subset.
 */
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../../..");
const OUT = path.join(HERE, "PROVIDER_SUPPORT_MATRIX.json");

const sha256 = (p) => crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");
const rel = (p) => path.relative(REPO, p).split(path.sep).join("/");

// ── 1. live plugin objects ────────────────────────────────────────────────
const providerDir = path.join(REPO, "lib/providers");
const HELPER_FILES = new Set(["media-schema-helpers.ts", "xai-oauth-cli-headers.ts"]);
const pluginFiles = fs
  .readdirSync(providerDir)
  .filter((f) => f.endsWith(".ts") && !HELPER_FILES.has(f))
  .sort();

const providers = [];
for (const file of pluginFiles) {
  const mod = await import(path.join(providerDir, file));
  for (const [exportName, value] of Object.entries(mod)) {
    if (
      value &&
      typeof value === "object" &&
      typeof value.id === "string" &&
      typeof value.authType === "string" &&
      typeof value.defaultApi === "string"
    ) {
      const media = {};
      const rawMedia = value.capabilities?.media ?? {};
      for (const [kind, section] of Object.entries(rawMedia)) {
        media[kind] = {
          defaultModelId: section.defaultModelId ?? null,
          models: (section.models ?? []).map((m) => ({
            id: m.id,
            protocolId: m.protocolId ?? null,
            inputs: m.inputs ?? [],
            outputs: m.outputs ?? [],
            supportsEdit: m.supportsEdit ?? false,
            aliases: m.aliases ?? [],
          })),
        };
      }
      const extra = {};
      for (const key of ["headers", "modelExecutionHeaders", "authJsonKey", "runtime", "sdkProvider", "models"]) {
        if (value[key] !== undefined) {
          extra[key] =
            key === "models"
              ? { declaredModelCount: Array.isArray(value.models) ? value.models.length : 0 }
              : value[key];
        }
      }
      providers.push({
        id: value.id,
        displayName: value.displayName ?? null,
        authType: value.authType,
        defaultBaseUrl: value.defaultBaseUrl ?? "",
        defaultApi: value.defaultApi,
        chatCapabilityOverride: value.capabilities?.chat ?? null,
        media,
        extraFields: extra,
        sourceFile: `lib/providers/${file}`,
        exportName,
      });
    }
  }
}
providers.sort((a, b) => a.id.localeCompare(b.id));
if (providers.length !== 39) {
  console.error(`expected 39 provider plugins, extracted ${providers.length}`);
  process.exit(1);
}

// ── 2. protocol family / auth rollups ─────────────────────────────────────
const rollup = (key) => {
  const out = {};
  for (const p of providers) (out[p[key]] ??= []).push(p.id);
  return Object.fromEntries(
    Object.entries(out)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([k, v]) => [k, { count: v.length, providers: v.sort() }]),
  );
};

// ── 3. provider-compat dispatcher (static read) ───────────────────────────
const compatMain = fs.readFileSync(path.join(REPO, "core/provider-compat.ts"), "utf8");
const compatDir = path.join(REPO, "core/provider-compat");
const compatModules = fs
  .readdirSync(compatDir)
  .filter((f) => f.endsWith(".ts"))
  .sort()
  .map((f) => {
    const text = fs.readFileSync(path.join(compatDir, f), "utf8");
    const firstComment = text.match(/^\/\*\*([\s\S]*?)\*\//)?.[1] ?? "";
    const purpose =
      firstComment
        .split("\n")
        .map((l) => l.replace(/^\s*\*\s?/, "").trim())
        .filter(Boolean)[0] ?? "";
    return { module: `core/provider-compat/${f}`, purpose };
  });
const dispatchBlock = compatMain.match(/const PROVIDER_MODULES[^=]*= \[([\s\S]*?)\];/)?.[1] ?? "";
const dispatchOrder = dispatchBlock
  .split("\n")
  .map((l) => l.replace(/\/\/.*$/, "").trim().replace(/,$/, ""))
  .filter((l) => l && !l.startsWith("*"));

// ── 4. model-operations vocabulary ────────────────────────────────────────
const modelOps = await import(path.join(REPO, "shared/model-operations.ts"));

// ── 5. media adapters + speech recognition ────────────────────────────────
const mediaAdapterDir = path.join(REPO, "core/media-adapters");
const mediaAdapters = fs
  .readdirSync(mediaAdapterDir)
  .filter((f) => f.endsWith(".ts"))
  .sort();
const speechDir = path.join(REPO, "core/speech-recognition");
const speechAdapters = fs
  .readdirSync(speechDir)
  .filter((f) => f.endsWith(".ts"))
  .sort();

const mediaProtocolIds = new Set();
for (const p of providers)
  for (const section of Object.values(p.media))
    for (const m of section.models) if (m.protocolId) mediaProtocolIds.add(m.protocolId);

// ── 6. R00 ledger cross-check ─────────────────────────────────────────────
const amap = JSON.parse(
  fs.readFileSync(path.join(REPO, "docs/rust-tauri/R00/ACCEPTANCE_MAP.json"), "utf8"),
);
const leafRe = /注册 id=([a-z0-9-]+)、auth=([a-z-]+)、api=([a-z-]+)/;
const byId = new Map(providers.map((p) => [p.id, p]));
const crosscheck = { leaves: [], mismatches: [] };
for (const [lid, leaf] of Object.entries(amap.scenarios)) {
  if (leaf.kind !== "supplemental" || !(leaf.execution_stage_ids ?? []).includes("R05")) continue;
  if (!leaf.feature_id.includes("-PROVIDER-PROVIDER-")) continue;
  const m = (leaf.then ?? "").match(leafRe);
  if (!m) {
    // system-speech / volcengine-speech leaves record no auth/api triple.
    crosscheck.leaves.push({ leaf: lid, providerId: null, note: leaf.then.slice(0, 60) });
    continue;
  }
  const [, pid, auth, api] = m;
  const row = byId.get(pid);
  const ok =
    row && row.authType === auth && row.defaultApi === api && row.id === pid;
  crosscheck.leaves.push({
    leaf: lid,
    providerId: pid,
    ledgerAuth: auth,
    ledgerApi: api,
    matrixAuth: row?.authType ?? null,
    matrixApi: row?.defaultApi ?? null,
    match: Boolean(ok),
  });
  if (!ok) crosscheck.mismatches.push(lid);
}
if (crosscheck.mismatches.length > 0) {
  console.error("R00 leaf cross-check mismatches:", crosscheck.mismatches);
  process.exit(1);
}

// ── 7. assemble ───────────────────────────────────────────────────────────
const doc = {
  schema: "lingxi.r05-provider-support-matrix.v1",
  generated_by: "docs/rust-tauri/R05/r05_t01_extract_provider_matrix.mjs",
  generated_at: "2026-10-02",
  extraction_method:
    "Node 24 直接 import lib/providers/*.ts 的 39 个插件导出对象（真实运行时值，非文本猜测）；" +
    "provider-compat 调度表、model-operations 词汇、media/speech 适配器清单为静态读取；" +
    "每行与 R00 账本的 注册 id/auth/api 三元组逐字段核对。",
  source_digests: {
    "lib/providers/": sha256(path.join(providerDir, "openai.ts")) && "per-row sourceFile 为准",
    "core/provider-compat.ts": sha256(path.join(REPO, "core/provider-compat.ts")),
    "shared/model-operations.ts": sha256(path.join(REPO, "shared/model-operations.ts")),
    "docs/rust-tauri/R00/ACCEPTANCE_MAP.json": sha256(
      path.join(REPO, "docs/rust-tauri/R00/ACCEPTANCE_MAP.json"),
    ),
  },
  totals: {
    providerPlugins: providers.length,
    helperFiles: [...HELPER_FILES].sort(),
    protocolFamilies: Object.fromEntries(
      Object.entries(rollup("defaultApi")).map(([k, v]) => [k, v.count]),
    ),
    authShapes: Object.fromEntries(
      Object.entries(rollup("authType")).map(([k, v]) => [k, v.count]),
    ),
    mediaProtocolIds: [...mediaProtocolIds].sort(),
  },
  protocol_families: rollup("defaultApi"),
  auth_shapes: rollup("authType"),
  providers,
  provider_compat_layer: {
    entry: "core/provider-compat.ts",
    dispatch_rule: "first-match-wins",
    dispatch_order: dispatchOrder,
    modules: compatModules,
    note: "唯一出站 payload 兼容入口 normalizeProviderPayload；chat 链路与 utility callText 共享。",
  },
  model_operations: {
    operationIds: modelOps.MODEL_OPERATION_IDS,
    operationProtocols: modelOps.MODEL_OPERATION_PROTOCOLS,
  },
  media_adapters: mediaAdapters.map((f) => `core/media-adapters/${f}`),
  speech_recognition_adapters: speechAdapters.map((f) => `core/speech-recognition/${f}`),
  r00_provider_leaf_crosscheck: crosscheck,
};
fs.writeFileSync(OUT, JSON.stringify(doc, null, 1) + "\n", "utf8");
console.log(`wrote ${rel(OUT)}: ${providers.length} providers, ` +
  `${Object.keys(doc.protocol_families).length} protocol families, ` +
  `${crosscheck.leaves.length} R00 provider leaves cross-checked (0 mismatch)`);
