/**
 * R00-T06｜合成 LINGXI_HOME 夹具工厂
 *
 * 产出完全合成的隔离 LINGXI_HOME（绝不含真实用户数据）：
 * - provider-catalog.json：唯一 provider 指向确定性本地 stub（127.0.0.1，
 *   isLocalBaseUrl 免 api_key；不配置任何真实供应商）。
 * - agents/lingxi/config.yaml：lib/config.example.yaml 的副本，models.chat 指向
 *   stub 模型（与 tests/server-composition-boundary.test.ts 同一做法）。
 * - user/preferences.json：setupComplete=true（跳过首启 onboarding，稳定样本）。
 * - agents/lingxi/sessions/：确定性合成会话 JSONL（scripts/lib/history-read-fixture.mjs
 *   的 buildLongRunFixtureBytes —— 与仓库既有基准同一语料，含 sha256 审计锚）。
 *
 * 提供 materializeHome()：每次测量样本前从 pristine 模板克隆出全新副本，
 * 保证每个样本的磁盘状态一致（steady-state）。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { sha256Bytes } from "./r00-t06-lib.mjs";
import { buildLongRunFixtureBytes } from "../lib/history-read-fixture.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");

export const FIXTURE_SPEC = {
  agentId: "lingxi",
  providerId: "bench-stub",
  modelId: "bench-fast",
  sessions: {
    "bench-long-1000.jsonl": 1000, // 长会话：1000 条消息（05 §6 最低负载）
    "bench-short-20.jsonl": 20,    // 短会话
  },
};

export function buildPristineHome({ dest, stubBaseUrl }) {
  fs.mkdirSync(dest, { recursive: true });

  // provider catalog → 确定性本地 stub；无真实供应商
  fs.writeFileSync(
    path.join(dest, "provider-catalog.json"),
    JSON.stringify({
      catalogVersion: 2,
      providers: {
        [FIXTURE_SPEC.providerId]: {
          base_url: stubBaseUrl,
          api: "openai-completions",
          // toolUse 契约：openai 方言 + message 工具结果（与 stub 实现的
          // chat/completions tool_calls 协议一致），否则会话工具集被砍到最小。
          models: [{
            id: FIXTURE_SPEC.modelId,
            toolUse: {
              supportsTools: true,
              dialect: "openai",
              toolResultFormat: "message",
              supportsParallelToolCalls: true,
            },
          }],
        },
      },
    }, null, 2),
  );

  // agent config：example 副本 + chat 模型指向 stub（YAML flow mapping，与
  // tests/server-composition-boundary.test.ts 的写法一致）
  const example = fs.readFileSync(path.join(ROOT, "lib", "config.example.yaml"), "utf8");
  const chatInline = `chat: {id: ${FIXTURE_SPEC.modelId}, provider: ${FIXTURE_SPEC.providerId}}`;
  const patched = example.replace(/^(\s*)chat:\s.*$/m, `$1${chatInline}`);
  if (patched === example || !patched.includes(chatInline)) {
    throw new Error("config.example.yaml 的 models.chat 段无法替换为 stub 模型");
  }
  const agentDir = path.join(dest, "agents", FIXTURE_SPEC.agentId);
  fs.mkdirSync(agentDir, { recursive: true });
  // 关闭记忆后台任务：避免 memory-ticker 在测量窗内对合成会话做滚动摘要
  // 编译/修复（后台写会引入噪声，且与被测负载无关）。其他配置逐字保留。
  const withMemoryOff = patched.replace(/^(\s*)enabled:\s*true\s*#?\s*记忆总开关.*$/m, "$1enabled: false");
  fs.writeFileSync(path.join(agentDir, "config.yaml"), withMemoryOff === patched ? patched : withMemoryOff);

  // preferences：跳过 onboarding
  fs.mkdirSync(path.join(dest, "user"), { recursive: true });
  fs.writeFileSync(path.join(dest, "user", "preferences.json"), JSON.stringify({
    setupComplete: true,
    primaryAgent: FIXTURE_SPEC.agentId,
  }, null, 2));

  // 合成会话语料
  const sessionsDir = path.join(agentDir, "sessions");
  fs.mkdirSync(sessionsDir, { recursive: true });
  const fileHashes = {};
  for (const [name, n] of Object.entries(FIXTURE_SPEC.sessions)) {
    const bytes = buildLongRunFixtureBytes(n);
    fs.writeFileSync(path.join(sessionsDir, name), bytes);
    fileHashes[name] = { messages: n, sha256: sha256Bytes(bytes), bytes: bytes.length };
  }

  return {
    homePath: dest,
    files: fileHashes,
    stubBaseUrl,
  };
}

/** 克隆 pristine 模板到新副本（每样本一致磁盘状态）。 */
export function cloneHome(pristineDir, tag) {
  const dest = fs.mkdtempSync(path.join(os.tmpdir(), `lingxi-r00t06-${tag}-`));
  const res = spawnSync("cp", ["-R", `${pristineDir}/.`, dest], { encoding: "utf8" });
  if (res.status !== 0) throw new Error(`cloneHome failed: ${res.stderr}`);
  return dest;
}
