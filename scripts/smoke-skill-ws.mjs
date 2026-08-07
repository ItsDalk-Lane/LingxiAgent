#!/usr/bin/env node
/**
 * smoke-skill-ws.mjs — WS-layer smoke test for the pure-skill-message gate.
 *
 * 背景：server/routes/chat.ts 的消息门禁（≈ L1981）曾经只认
 * text/images/videos/audios，不含 skills。用户点快捷技能按钮发送的纯
 * skillBadge 消息 text 为空、只有 msg.skills——这种消息会落进门禁的
 * 空洞（if 块不执行，无 else，async IIFE 直接结束），被静默丢弃：用户
 * 看到消息气泡但永远没有响应，新会话里还会留下无法进入的空记录。
 *
 * 这个脚本端到端验证修复：spawn 一个 dev server（复用 ~/.lingxi-dev，
 * 继承已配置的 provider，避免 throwaway home 的模型可用性问题），创建
 * 一个一次性会话，用 WS 客户端发送 text:"" + skills:["x"] 的纯技能消息，
 * 断言 server 回出 session_user_message 且文本里含 "[Use skill: x]"——
 * 证明门禁放行、skills 拼接生效。测试会话文件在结束时删除。
 *
 * 与 tests/chat-route-skill-message.test.ts 的分工：那个是单元测试
 * （mock hub.send，验证拼字符串），本脚本打通鉴权 + 身份解析 + 门禁 +
 * 拼接的完整 WS 链路。两者互补，不重复。
 *
 * 用法：node scripts/smoke-skill-ws.mjs
 * 退出码 0=通过，1=失败。
 *
 * CI：需要本机有可用模型（如 ollama on localhost:11434）。曾尝试用
 * LINGXI_SMOKE_NO_MODEL env gate 让无模型环境也跑（core/session-coordinator.ts），
 * 但消息处理链路最终触达 pi SDK 内部的模型凭证检查（session.prompt），
 * 该检查在 session_user_message 之前抛出 "No API key found"，无法在不
 * 侵入 SDK 的前提下绕过。因此本脚本定位为本地/带模型的自托管 runner
 * smoke，不进无模型的 CI 矩阵（CI 逻辑回归由 chat-route-skill-message
 * 单元测试守卫）。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { WebSocket } from "ws";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const SERVER_ENTRY = path.join(ROOT, "server", "main-full.ts");

// 复用 dev home：throwaway home 里 ModelRegistry 的可用性判定依赖运行时
// checkAuth（pi SDK 内部），同份 models.json 在 dev home 有模型、在
// throwaway 是 0，难以靠注入文件复现。dev home 本就是开发数据目录，
// 复用它既继承 provider 配置，又避免重复造模型注入。
function devLingxiHome() {
  const fromEnv = process.env.LINGXI_HOME;
  if (fromEnv) return fromEnv;
  return path.join(os.homedir(), ".lingxi-dev");
}

// ── 进程生命周期工具（沿用 smoke-open-server.mjs 的清理语义）──────────────
function waitForExit(child, { timeoutMs }) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  }
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`timed out after ${timeoutMs}ms waiting for process exit`));
    }, timeoutMs);
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal });
    });
  });
}

async function terminateChild(child, { timeoutMs = 10_000 } = {}) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  try {
    await waitForExit(child, { timeoutMs });
  } catch {
    child.kill("SIGKILL");
    await waitForExit(child, { timeoutMs: 5_000 }).catch(() => {});
  }
}

// ── spawn dev server（复用 dev home 的 provider 配置）────────────────────
function spawnDevServer({ lingxiHome }) {
  let stderrBuf = "";
  let stdoutBuf = "";
  const child = spawn(process.execPath, [SERVER_ENTRY], {
    cwd: ROOT,
    env: {
      ...process.env,
      LINGXI_HOME: lingxiHome,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk) => { stdoutBuf += chunk.toString(); });
  child.stderr.on("data", (chunk) => { stderrBuf += chunk.toString(); });
  return { child, getStdout: () => stdoutBuf, getStderr: () => stderrBuf };
}

async function readServerInfo(lingxiHome, { child, getStderr }) {
  const serverInfoPath = path.join(lingxiHome, "server-info.json");
  // 记录 spawn 前的 mtime：共享 LINGXI_HOME 下 server-info.json 会被上一次
  // server 残留，必须等到"本次 spawn 之后的新写入"才算拿到本进程的端口。
  const spawnMtime = fs.existsSync(serverInfoPath)
    ? fs.statSync(serverInfoPath).mtimeMs
    : 0;

  // 阶段一：等文件 mtime 变化（本进程的 server 写入），或进程退出报错。
  await Promise.race([
    (async () => {
      const deadline = Date.now() + 60_000;
      while (Date.now() < deadline) {
        if (child.exitCode !== null || child.signalCode !== null) break;
        if (fs.existsSync(serverInfoPath)) {
          const m = fs.statSync(serverInfoPath).mtimeMs;
          if (m > spawnMtime) return;
        }
        await new Promise((r) => setTimeout(r, 200));
      }
    })(),
    waitForExit(child, { timeoutMs: 60_000 }).then(({ code, signal }) => {
      throw new Error(
        `[smoke-skill-ws] server exited before publishing server-info.json (code=${code}, signal=${signal})\n--- stderr ---\n${getStderr()}`,
      );
    }),
  ]);

  // 阶段二：文件已更新，读一个含 port+token 的版本（flush 中途可能解析失败，重试）。
  for (let attempt = 0; attempt < 30; attempt++) {
    try {
      const info = JSON.parse(fs.readFileSync(serverInfoPath, "utf-8"));
      if (info.port && info.token) return info;
    } catch { /* 文件 flush 中途或正在重写，稍后重试 */ }
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(`[smoke-skill-ws] server-info.json never reported a valid port+token at ${serverInfoPath}`);
}

// ── HTTP: 创建会话，拿 sessionPath + agentId ─────────────────────────────
async function createSession({ host, port, token }) {
  const url = `http://${host === "0.0.0.0" ? "127.0.0.1" : host}:${port}/api/sessions/new`;
  const res = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ memoryEnabled: false }),
  });
  if (res.status !== 200) {
    const body = await res.text().catch(() => "<unreadable>");
    if (res.status === 409 && body.includes("no_available_model")) {
      throw new Error(
        `[smoke-skill-ws] createSession 409: 无可用模型。本 smoke 需要本机配置了可用模型（如 ollama on localhost:11434）。\n`
        + `请在设置中配置模型后重试，或在有模型的机器/runner 上运行。\nserver 响应: ${body}`,
      );
    }
    throw new Error(`[smoke-skill-ws] POST ${url} returned ${res.status}, expected 200. body: ${body}`);
  }
  const body = await res.json();
  if (!body.path) {
    throw new Error(`[smoke-skill-ws] /api/sessions/new response missing path: ${JSON.stringify(body)}`);
  }
  return { sessionPath: body.path, agentId: body.agentId || null };
}

// ── WS: 发送纯 skills 消息，收集响应 ─────────────────────────────────────
function sendSkillMessage({ host, port, token, sessionPath, agentId, skillName }) {
  return new Promise((resolve, reject) => {
    const wsHost = host === "0.0.0.0" ? "127.0.0.1" : host;
    const url = `ws://${wsHost}:${port}/ws`;
    const ws = new WebSocket(url, { headers: { Authorization: `Bearer ${token}` } });
    const received = [];
    let settled = false;

    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { ws.close(); } catch {}
      resolve({ ...result, received });
    };

    const timer = setTimeout(() => {
      finish({ kind: "timeout" });
    }, 5000);

    ws.on("open", () => {
      // 纯技能消息：text 为空，只有 skills。这正是修复前会被静默丢弃的形态。
      const msg = { type: "prompt", text: "", skills: [skillName], sessionPath, agentId };
      ws.send(JSON.stringify(msg));
    });

    ws.on("message", (data) => {
      const str = data.toString();
      let parsed;
      try { parsed = JSON.parse(str); } catch { parsed = null; }
      received.push(parsed || str);

      // session_user_message 出现即证明门禁放行 + skills 拼接生效，提前收尾。
      if (parsed?.type === "session_user_message") {
        finish({ kind: "accepted", userMessage: parsed });
      }
      // 身份/鉴权类错误：立刻失败，不等超时。
      if (parsed?.type === "error") {
        finish({ kind: "error", error: parsed });
      }
    });

    ws.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      reject(new Error(`[smoke-skill-ws] WS error: ${err.message}`));
    });
  });
}

// ── 主流程 ───────────────────────────────────────────────────────────────
async function main() {
  const lingxiHome = devLingxiHome();
  console.log(`[smoke-skill-ws] LINGXI_HOME=${lingxiHome}`);
  const sessionsDir = path.join(lingxiHome, "agents", "lingxi", "sessions");
  // 记录 spawn 前的快照：createSession 有时会先落实挂起草稿再建返回的会话，
  // 产生两个新文件。按 mtime 清理本次运行期间新建的所有会话文件最稳妥。
  const spawnTime = Date.now();
  const { child, getStderr } = spawnDevServer({ lingxiHome });

  try {
    const info = await readServerInfo(lingxiHome, { child, getStderr });
    console.log(`[smoke-skill-ws] server ready on ${info.host}:${info.port}`);

    const created = await createSession(info);
    console.log(`[smoke-skill-ws] session created: ${path.basename(created.sessionPath)} (agent=${created.agentId})`);

    const SKILL = "smoke-test-skill";
    const result = await sendSkillMessage({ ...info, ...created, skillName: SKILL });

    // ── 断言 ─────────────────────────────────────────────────────────────
    if (result.kind === "error") {
      throw new Error(
        `[smoke-skill-ws] FAIL: server rejected pure-skill message with error:\n${JSON.stringify(result.error, null, 2)}\n\n`
        + "门禁未放行——检查 server/routes/chat.ts 的 prompt 分支是否包含 msg.skills?.length。",
      );
    }
    if (result.kind !== "accepted") {
      throw new Error(
        `[smoke-skill-ws] FAIL: no session_user_message within timeout (${result.kind}).\n`
        + `received ${result.received.length} messages:\n`
        + result.received.map((m, i) => `  [${i}] ${typeof m === "string" ? m : JSON.stringify(m)}`).join("\n"),
      );
    }

    const text = result.userMessage?.message?.text || "";
    if (!text.includes(`[Use skill: ${SKILL}]`)) {
      throw new Error(
        `[smoke-skill-ws] FAIL: session_user_message.text does not contain skill tag.\n`
        + `expected to include "[Use skill: ${SKILL}]"\nactual: ${JSON.stringify(text)}`,
      );
    }

    console.log(`[smoke-skill-ws] PASS: pure-skill message accepted, text="${text.trim()}"`);
    console.log("[smoke-skill-ws] 门禁放行 + skills 拼接验证通过");
  } finally {
    await terminateChild(child);
    // 清理本次运行期间新建的会话文件（manifest 对缺失文件有兜底，stale 记录无害）。
    cleanupSessionsSince(sessionsDir, spawnTime);
  }
}

function cleanupSessionsSince(sessionsDir, sinceMs) {
  if (!fs.existsSync(sessionsDir)) return;
  let removed = 0;
  for (const name of fs.readdirSync(sessionsDir)) {
    if (!name.endsWith(".jsonl")) continue;
    const full = path.join(sessionsDir, name);
    try {
      const st = fs.statSync(full);
      if (st.mtimeMs >= sinceMs) {
        fs.rmSync(full, { force: true });
        removed++;
      }
    } catch { /* 文件已被删或不可访问，忽略 */ }
  }
  if (removed > 0) console.log(`[smoke-skill-ws] cleaned ${removed} test session file(s)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error(err instanceof Error ? err.message : String(err));
    process.exit(1);
  });
}
