import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { performance } from "node:perf_hooks";
import { LocalModelsSubsystem, type LocalModelRegistryEntry } from "../lib/local-models/index.ts";

const home = process.env.LINGXI_HOME ? path.resolve(process.env.LINGXI_HOME) : "";
if (!home) {
  console.error("请显式设置 LINGXI_HOME；基准工具不会猜测或创建用户数据目录。");
  process.exit(2);
}

const preferencesPath = path.join(home, "user", "preferences.json");
const preferences = readJson(preferencesPath);
const subsystem = new LocalModelsSubsystem({
  lingxiHome: home,
  getPreferences: () => preferences,
  savePreferences: () => { throw new Error("基准工具禁止修改设置"); },
});
const controller = new AbortController();
const iterations = boundedInteger(process.env.LOCAL_MODEL_BENCH_ITERATIONS, 1, 20, 3);
const results: Record<string, unknown> = {};

try {
  await subsystem.initialize({ signal: controller.signal });
  const installed = subsystem.registry.snapshot().models;
  results.embedding = await runCategory("embedding", installed, async (model, signal) => {
    await subsystem.runtime.embed({
      model: ref(model),
      texts: ["灵犀本地模型基准", "offline local model benchmark"],
      signal,
      priority: "batch",
    });
  });
  results.ocr = await runFileCategory("ocr", "LOCAL_MODEL_BENCH_IMAGE", installed, async (model, filePath, signal) => {
    await subsystem.runtime.ocr({
      model: ref(model),
      image: fs.readFileSync(filePath),
      mime: mimeForImage(filePath),
      signal,
      priority: "batch",
    });
  });
  results.stt = await runFileCategory("stt", "LOCAL_MODEL_BENCH_AUDIO", installed, async (model, filePath, signal) => {
    await subsystem.runtime.transcribe({ model: ref(model), filePath, signal, priority: "batch" });
  });
  results.tts = await runCategory("tts", installed, async (model, signal) => {
    await subsystem.runtime.synthesize({
      model: ref(model),
      text: "这是灵犀本地语音合成基准。",
      signal,
      priority: "batch",
    });
  });
  console.log(JSON.stringify({
    generatedAt: new Date().toISOString(),
    platform: `${process.platform}-${process.arch}`,
    node: process.version,
    iterations,
    results,
  }, null, 2));
} finally {
  controller.abort();
  await subsystem.dispose();
}

async function runCategory(
  category: LocalModelRegistryEntry["category"],
  installed: LocalModelRegistryEntry[],
  invoke: (model: LocalModelRegistryEntry, signal: AbortSignal) => Promise<void>,
) {
  const model = installed.find((entry) => entry.category === category);
  if (!model) return { status: "NOT_EXECUTED", reason: "no installed model" };
  return measure(model, invoke);
}

async function runFileCategory(
  category: LocalModelRegistryEntry["category"],
  envName: string,
  installed: LocalModelRegistryEntry[],
  invoke: (model: LocalModelRegistryEntry, filePath: string, signal: AbortSignal) => Promise<void>,
) {
  const filePath = process.env[envName] ? path.resolve(process.env[envName]!) : "";
  if (!filePath || !fs.statSync(filePath, { throwIfNoEntry: false })?.isFile()) {
    return { status: "NOT_EXECUTED", reason: `${envName} is not a readable file` };
  }
  const model = installed.find((entry) => entry.category === category);
  if (!model) return { status: "NOT_EXECUTED", reason: "no installed model" };
  return measure(model, (entry, signal) => invoke(entry, filePath, signal));
}

async function measure(
  model: LocalModelRegistryEntry,
  invoke: (model: LocalModelRegistryEntry, signal: AbortSignal) => Promise<void>,
) {
  await subsystem.runtime.unload(ref(model)).catch(() => false);
  const beforeRssMb = rssMb();
  const coldStarted = performance.now();
  await invoke(model, new AbortController().signal);
  const coldMs = performance.now() - coldStarted;
  const warmMs: number[] = [];
  for (let index = 0; index < iterations; index += 1) {
    const started = performance.now();
    await invoke(model, new AbortController().signal);
    warmMs.push(performance.now() - started);
  }
  return {
    status: "PASS",
    model: `local:${model.id}@${model.quant}@${model.version}`,
    runtime: `${model.runtimeId}@${model.runtimeVersion}`,
    coldMs: round(coldMs),
    warmMs: warmMs.map(round),
    warmAverageMs: round(warmMs.reduce((sum, value) => sum + value, 0) / warmMs.length),
    processRssDeltaMb: round(rssMb() - beforeRssMb),
  };
}

function ref(model: LocalModelRegistryEntry) {
  return { id: model.id, quant: model.quant, manifestVersion: model.version };
}

function readJson(filePath: string): Record<string, unknown> {
  try {
    const value = JSON.parse(fs.readFileSync(filePath, "utf8"));
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
  } catch {
    return {};
  }
}

function mimeForImage(filePath: string): string {
  const extension = path.extname(filePath).toLowerCase();
  return extension === ".jpg" || extension === ".jpeg" ? "image/jpeg"
    : extension === ".webp" ? "image/webp"
      : extension === ".tif" || extension === ".tiff" ? "image/tiff"
        : "image/png";
}

function boundedInteger(value: string | undefined, min: number, max: number, fallback: number): number {
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= min && parsed <= max ? parsed : fallback;
}

function rssMb(): number {
  return process.memoryUsage().rss / (1024 * 1024);
}

function round(value: number): number {
  return Math.round(value * 100) / 100;
}
