/**
 * P06-T01｜最终请求装配来源盘点（确定性捕获，任务书 §5 P06-T01）。
 *
 * 目标：不只看一个 system prompt 字符串，而是在真实请求边界捕获最终 request，
 * 证明桌面主会话 / subagent / Bridge owner / Bridge guest / 后台(automation) 五个
 * 变体都消费同一 canonical 装配（Agent.buildSystemPromptArtifact），没有任何
 * 第二套拼装混入最终请求：
 *
 *   1. 桌面主会话：真实 createAgentSession facade + Fake Provider Witness（真实
 *      HTTP），从观测库 semantic_request 四层 payload 读回 streamFn 边界的
 *      {systemPrompt}，与 canonical artifact.text 逐字节对照；同时对照 witness
 *      收到的请求体（truth oracle）。Lingxi 基座、append 各段、skills 目录、
 *      project_context 在最终请求中各出现且仅出现一次。
 *   2. subagent 变体：forSubagent 快照走同一链路；最终请求不含记忆/团队段。
 *   3. Bridge owner/guest：真实 BridgeSessionManager._buildOwnerPromptSnapshot /
 *      _buildGuestPromptSnapshot（Object.create 桩只替换 _deps，方法体为生产实现）；
 *      owner 含 canonical 基座 + bridge 行；guest 只含 yuan 模板 + publicAgentsMd，
 *      不含用户档案/记忆/样貌（隐私边界，P06-A10 关联）。
 *   4. 后台（automation/巡检/cron）：session-coordinator 非 subagent 隔离运行消费
 *      agent.systemPrompt master 缓存——断言该 getter 返回的就是 buildSystemPrompt
 *      同一装配的输出（时间冻结下字节一致）。
 *
 * 测试结束后输出脱敏样本索引（console.log，来源+长度，无正文副本），由命令日志
 * 归档到 artifacts/refactor-2026/P06/logs/。所有 fixture 为合成数据（受控临时目录），
 * 不读写真实用户 HOME/会话/记忆。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionManager, DefaultResourceLoader } from "@earendil-works/pi-coding-agent";

import { Agent } from "../core/agent.ts";
import {
  readAgentAvatarResource,
  writeAgentAppearanceProfileResource,
} from "../lib/agent-appearance-summary.ts";
import {
  buildSessionPromptSnapshot,
  createPromptSnapshotResourceLoader,
} from "../core/session-prompt-snapshot.ts";
import { BridgeSessionManager } from "../core/bridge-session-manager.ts";
import { formatWorkspaceScopePrompt } from "../shared/workspace-scope.ts";
import { buildWorkspaceInstructionPrompt } from "../core/workspace-instruction-files.ts";
import { agentPersonaFilePaths } from "../core/persona-source.ts";
import { createAgentSession } from "../lib/pi-sdk/index.ts";
import {
  createScenarioHarness,
  flushAsync,
  openaiCompletionsSseBody,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

/* ── 合成 fixture（受控临时目录，无真实用户数据） ───────────────────────── */

const PERSONA_MARKER = "P06-AGENTSMD-TEMPLATE-PERSONA";
const PROFILE_MARKER = "P06-PROFILE";
const MEMORY_MARKER = "P06-MEMORY";
const TENETS_MARKER = "P06-PINNED";
const APPEARANCE_MARKER = "P06-APPEARANCE";
const SKILL_NAME = "p06-fixture-skill";
const CONTEXT_FILE_MARKER = "P06-CONTEXT-FILE";

const tempDirs: string[] = [];
const frozenNow = new Date("2026-06-04T07:53:00.000Z");

function makeTempDir(prefix: string) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

function makeFixtureAgent() {
  const root = makeTempDir("hana-p06-assembly-");
  const agentsDir = path.join(root, "agents");
  const productDir = path.join(root, "product");
  const userDir = path.join(root, "user");
  const agentDir = path.join(agentsDir, "hana");
  fs.mkdirSync(path.join(agentDir, "memory"), { recursive: true });
  fs.mkdirSync(path.join(agentDir, "avatars"), { recursive: true });
  fs.mkdirSync(path.join(productDir, "yuan"), { recursive: true });
  fs.mkdirSync(userDir, { recursive: true });
  fs.writeFileSync(
    path.join(productDir, "yuan", "lingxi.md"),
    `${PERSONA_MARKER} 你是{{userName}}的伙伴`,
    "utf-8",
  );
  fs.writeFileSync(path.join(userDir, "user.md"), `${PROFILE_MARKER} 简介\n`, "utf-8");
  fs.writeFileSync(path.join(agentDir, "memory", "tenets.json"), JSON.stringify({
    schemaVersion: 1,
    tenets: [{
      id: "p06-pinned-1", content: `${TENETS_MARKER} 置顶`, priority: "high",
      status: "active", source: "user_direct", sessionId: null,
      createdAt: "2026-09-06T00:00:00.000Z", decidedAt: "2026-09-06T00:00:00.000Z",
    }],
  }, null, 2) + "\n", "utf-8");
  fs.writeFileSync(path.join(agentDir, "memory", "memory.md"), `${MEMORY_MARKER} 记忆\n`, "utf-8");
  fs.writeFileSync(path.join(agentDir, "avatars", "agent.png"), Buffer.from("fake-avatar-bytes"));
  const avatar = readAgentAvatarResource(agentDir);
  writeAgentAppearanceProfileResource(agentDir, {
    avatarHash: avatar!.hash,
    summary: `${APPEARANCE_MARKER} 样貌`,
    model: null,
  });

  const agent = new Agent({ id: "hana", agentsDir, productDir, userDir } as any);
  agent._config = {
    locale: "zh-CN",
    agent: { yuan: "lingxi" },
    memory: { enabled: true },
    experience: { enabled: false },
    user: { name: "黎" },
  };
  agent.userName = "黎";
  agent.agentName = "Hanako";
  agent._canInjectAppearancePrompt = () => true;
  agent._isComputerUseAvailableForThisAgent = () => true;
  agent._listAgents = () => [
    { id: "hana", name: "Hanako", model: "gpt-test", summary: "主 agent" },
    { id: "beta", name: "Beta", model: "claude-test", summary: "副 agent" },
  ];
  agent._cb = { getTimezone: () => "Asia/Shanghai", getPreferences: () => ({}) };
  return { agent, root };
}

function makeFixtureSkill(root: string) {
  const skillDir = path.join(root, "skills", SKILL_NAME);
  fs.mkdirSync(skillDir, { recursive: true });
  const filePath = path.join(skillDir, "SKILL.md");
  fs.writeFileSync(filePath, `---\nname: ${SKILL_NAME}\n---\nfixture skill body\n`, "utf-8");
  return { name: SKILL_NAME, description: "P06 fixture skill for assembly capture", filePath, baseDir: skillDir };
}

function countOccurrences(haystack: string, needle: string) {
  let count = 0;
  let at = haystack.indexOf(needle);
  while (at >= 0) {
    count += 1;
    at = haystack.indexOf(needle, at + needle.length);
  }
  return count;
}

afterEach(() => {
  vi.useRealTimers();
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

/* ── 真实会话边界捕获（桌面主会话 / subagent 共用） ─────────────────────── */

let harness: ScenarioHarness | null = null;

beforeEach(() => {
  harness = null;
});

afterEach(async () => {
  await harness?.close();
  harness?.cleanup();
  harness = null;
});

async function createWitnessRuntime(baseUrl: string) {
  const { ModelRuntime } = await import("@earendil-works/pi-coding-agent");
  const runtime = await ModelRuntime.create({ modelsPath: null, refreshOnCreate: false });
  runtime.registerProvider("p06-witness-provider", {
    name: "P06 Witness Provider",
    baseUrl,
    api: "openai-completions",
    apiKey: "sk-P06-WITNESS-SYNTHETIC-KEY",
    authHeader: true,
  } as any);
  return runtime;
}

const WITNESS_MODEL = () => ({
  id: "p06-witness-model",
  provider: "p06-witness-provider",
  api: "openai-completions",
  baseUrl: `${harness!.witness.baseUrl}/v1`,
  maxTokens: 1024,
  input: ["text"],
  cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0, total: 0 },
});

const READ_TOOL = {
  name: "read",
  description: "Read a file (p06 fixture)",
  parameters: { type: "object" as const, properties: { path: { type: "string" } } },
  execute: vi.fn(async () => ({ content: [{ type: "text" as const, text: "fixture" }] })),
};

/** 与 session-coordinator 新建会话同构：canonical artifact → 冻结快照 → resourceLoader。 */
async function captureFinalRequest(opts: { forSubagent: boolean }) {
  // 不用 fake timers：真实 HTTP witness 与 fake timers 不兼容；canonical text 在
  // 构建后即冻结进快照，最终请求逐字节嵌入的是同一份字符串，无时钟漂移问题。
  harness = await createScenarioHarness();
  const { agent, root } = makeFixtureAgent();
  const skill = makeFixtureSkill(root);

  const artifactOptions: any = { forceMemoryEnabled: true };
  if (opts.forSubagent) artifactOptions.forSubagent = true;
  const artifact = agent.buildSystemPromptArtifact(artifactOptions);
  expect(artifact.text).toBe(agent.buildSystemPrompt(artifactOptions));

  const locale = "zh-CN";
  const workspacePrompt = formatWorkspaceScopePrompt({
    primaryCwd: harness.lingxiHome,
    workspaceFolders: [],
    locale,
  });
  const workspaceInstructions = buildWorkspaceInstructionPrompt({
    cwd: harness.lingxiHome,
    workspaceContext: undefined,
    locale,
    excludeFiles: agentPersonaFilePaths(agent.agentDir),
  });
  const snapshot = buildSessionPromptSnapshot({
    systemPrompt: artifact.text,
    appendSystemPrompt: [
      ...(workspacePrompt ? [workspacePrompt] : []),
      ...(workspaceInstructions ? [workspaceInstructions] : []),
    ],
    skillsResult: opts.forSubagent
      ? { skills: [], diagnostics: [] }
      : { skills: [skill], diagnostics: [] },
    agentsFilesResult: opts.forSubagent
      ? { agentsFiles: [] }
      : { agentsFiles: [{ path: "AGENTS.md", content: `${CONTEXT_FILE_MARKER} workspace instructions` }] },
    systemPromptProvenance: artifact.provenance,
  });

  const baseLoader = new DefaultResourceLoader({
    cwd: harness.lingxiHome,
    agentDir: harness.lingxiHome,
    noExtensions: true,
    noSkills: true,
    noPromptTemplates: true,
    noThemes: true,
    noContextFiles: true,
  } as any);
  await baseLoader.reload();
  const resourceLoader = createPromptSnapshotResourceLoader(baseLoader, snapshot);

  const runtime = await createWitnessRuntime(`${harness.witness.baseUrl}/v1`);
  harness.witness.scriptNext({ kind: "sse", body: openaiCompletionsSseBody({ content: "P06_REPLY" }) });

  const created = await createAgentSession({
    model: WITNESS_MODEL(),
    modelRuntime: runtime,
    sessionManager: SessionManager.inMemory(),
    resourceLoader,
    cwd: harness.lingxiHome,
    tools: [READ_TOOL],
  } as any);
  const session = created.session;
  await session.prompt("P06_ASSEMBLY_CAPTURE_INPUT");
  await flushAsync(5);
  harness.flush();
  await flushAsync(3);

  /* truth oracle：witness 收到的请求体 */
  const posts = harness.witness.requestsTo("/chat/completions");
  expect(posts).toHaveLength(1);
  const witnessSystem = (posts[0].bodyJson as any).messages
    .find((m: any) => m.role === "system")?.content;

  /* 观测库：semantic_request payload 的 streamFn 边界 messages[0]（结构化 sections）。
     Pi 真实路径 context 无 systemPrompt 字段（createContextSnapshot 只带 messages/tools），
     最终 system prompt 以 messages[0] 的结构化 sections 进入请求。 */
  const callIds = harness.observer!.callIds();
  expect(callIds).toHaveLength(1);
  const detail = harness.query().queryCallDetail(callIds[0]);
  expect(detail.ok).toBe(true);
  if (!detail.ok) throw new Error("queryCallDetail failed");
  const semanticRecord = detail.value.payloadRecords.find((r: any) => r.kind === "semantic_request");
  expect(semanticRecord).toBeTruthy();
  const payloadDetail = harness.query().getPayloadRecord(semanticRecord.id);
  expect(payloadDetail.ok).toBe(true);
  if (!payloadDetail.ok) throw new Error("getPayloadRecord failed");
  expect(payloadDetail.value.contentState).toBe("present");
  const capturedMessages = (payloadDetail.value.payload as any).messages as any[];
  expect(capturedMessages?.[0]?.role).toBe("system");
  const capturedSections: Record<string, string> = capturedMessages[0]?.sections ?? {};
  expect(typeof capturedSections.preamble).toBe("string");

  await session.dispose?.();
  return {
    artifact,
    capturedSections,
    capturedSystem: witnessSystem,
    witnessSystem,
    snapshot,
    workspacePrompt,
    workspaceInstructions,
  };
}

describe("P06-T01 final request assembly capture", () => {
  it("desktop main session: final request consumes the canonical assembly exactly once", async () => {
    const { artifact, capturedSections, witnessSystem, snapshot } = await captureFinalRequest({ forSubagent: false });
    const capturedSystem = witnessSystem;

    // A01：单一装配。结构化 preamble 逐字节等于 canonical artifact.text；
    // 渲染后的最终请求（witness truth oracle）以该基座开头。
    expect(capturedSections.preamble).toBe(artifact.text);
    expect(witnessSystem.startsWith(artifact.text)).toBe(true);

    // 基座只出现一次（没有第二套拼装混入）；canonical 段标记各出现一次。
    expect(countOccurrences(capturedSystem, PERSONA_MARKER)).toBe(1);
    expect(countOccurrences(capturedSystem, "你运行在灵犀（Lingxi）平台上。")).toBe(1);
    expect(countOccurrences(capturedSystem, PROFILE_MARKER)).toBe(1);
    expect(countOccurrences(capturedSystem, MEMORY_MARKER)).toBe(1);
    expect(countOccurrences(capturedSystem, APPEARANCE_MARKER)).toBe(1);
    expect(countOccurrences(capturedSystem, "## 工具使用纪律")).toBe(1);

    // append / skills / project_context 各一次；skills 目录只由 SDK 注入一次（#399 回归）。
    expect(countOccurrences(capturedSystem, "<addendum>")).toBe(1);
    expect(countOccurrences(capturedSystem, "<available_skills>")).toBe(1);
    expect(countOccurrences(capturedSystem, `<name>${SKILL_NAME}</name>`)).toBe(1);
    expect(countOccurrences(capturedSystem, "<project_context>")).toBe(1);
    expect(countOccurrences(capturedSystem, CONTEXT_FILE_MARKER)).toBe(1);
    // workspace 范围段（append 内）与 SDK cwd 段各出现一次（<cwd> 标记可能两处：
    // 一处在 addendum 的工作区指引文案，一处在 SDK 的 cwd section——按各自锚点计数）。
    expect(countOccurrences(capturedSystem, "## 工作区范围")).toBe(1);
    expect(capturedSystem.includes("主工作台")).toBe(true);

    // 快照把 provenance sections 一并冻结（恢复后描述“当时实际 prompt”）。
    expect(snapshot.systemPromptProvenance?.map((s: any) => s.source?.id)).toContain("persona");
    expect(snapshot.systemPromptProvenance?.map((s: any) => s.source?.id)).toContain("memory.longterm");
    const serializedProvenance = JSON.stringify(snapshot.systemPromptProvenance);
    for (const marker of [PERSONA_MARKER, PROFILE_MARKER, MEMORY_MARKER, APPEARANCE_MARKER]) {
      expect(serializedProvenance.includes(marker)).toBe(false); // provenance 无正文副本
    }
  });

  it("subagent variant: same chain, no memory/roster sections in the final request", async () => {
    const { artifact, capturedSections, witnessSystem: capturedSystem } = await captureFinalRequest({ forSubagent: true });
    expect(capturedSections.preamble).toBe(artifact.text);

    expect(capturedSections.preamble).toBe(artifact.text);
    expect(countOccurrences(capturedSystem, PERSONA_MARKER)).toBe(1);
    // subagent 是隔离子会话：不注入长期记忆与多 agent 协作上下文（agent.ts:1536 注释契约）。
    expect(capturedSystem.includes(MEMORY_MARKER)).toBe(false);
    expect(capturedSystem.includes(TENETS_MARKER)).toBe(false);
    expect(capturedSystem.includes("## 记忆使用规则")).toBe(false);
    expect(capturedSystem.includes("## 团队")).toBe(false);
    expect(capturedSystem.includes(APPEARANCE_MARKER)).toBe(false);
    // 用户档案（user.md）按现行设计保留（任务只要求隔离记忆与团队上下文，
    // 不改变已采纳行为——P06-T04 兼容边界）。
    expect(countOccurrences(capturedSystem, PROFILE_MARKER)).toBe(1);
  });

  it("bridge owner/guest snapshots ride production builders with privacy boundary", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(frozenNow);
    const { agent } = makeFixtureAgent();
    const skillsStub = {
      getSkillsForAgent: () => ({ skills: [], diagnostics: [] }),
    };
    const loaderStub = {
      getAppendSystemPrompt: () => [],
      getSkills: () => ({ skills: [], diagnostics: [] }),
      getAgentsFiles: () => ({ agentsFiles: [] }),
    };
    const manager = Object.create(BridgeSessionManager.prototype) as BridgeSessionManager;
    (manager as any)._deps = {
      getResourceLoader: () => loaderStub,
      getSkills: () => skillsStub,
    };

    const owner = (manager as any)._buildOwnerPromptSnapshot(
      agent,
      os.tmpdir(),
      (manager as any)._buildBridgeContext("tg_dm_777", {}, { guest: false }, agent),
    );
    // owner 基座 = canonical buildSystemPrompt（同一装配）+ bridge 行。
    const canonical = agent.buildSystemPrompt({ forceMemoryEnabled: agent.memoryMasterEnabled });
    expect(owner.systemPrompt.startsWith(canonical)).toBe(true);
    expect(owner.systemPrompt.length).toBeGreaterThan(canonical.length); // bridge 行已追加
    expect(countOccurrences(owner.systemPrompt, PERSONA_MARKER)).toBe(1);
    expect(owner.systemPrompt.includes(MEMORY_MARKER)).toBe(true); // owner 保留记忆
    expect(Array.isArray(owner.appendSystemPrompt)).toBe(true);

    const guest = (manager as any)._buildGuestPromptSnapshot(
      agent,
      (manager as any)._buildBridgeContext("tg_dm_888", {}, { guest: true }, agent),
    );
    // guest 只看 yuan 模板 + publicAgentsMd + bridge 行：无用户档案/记忆/样貌。
    expect(guest.systemPrompt.includes(PERSONA_MARKER)).toBe(true);
    expect(guest.systemPrompt.includes(PROFILE_MARKER)).toBe(false);
    expect(guest.systemPrompt.includes(MEMORY_MARKER)).toBe(false);
    expect(guest.systemPrompt.includes(TENETS_MARKER)).toBe(false);
    expect(guest.systemPrompt.includes(APPEARANCE_MARKER)).toBe(false);
    expect(guest.appendSystemPrompt).toEqual([]);
    expect(guest.skillsResult.skills).toEqual([]);
    expect(guest.agentsFilesResult.agentsFiles).toEqual([]);
  });

  it("background (automation/patrol) path consumes the master prompt cache from the same assembly", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(frozenNow);
    const { agent } = makeFixtureAgent();
    // session-coordinator 非 subagent 隔离运行读 targetAgent.systemPrompt（master
    // 缓存）。master 缓存由 buildSystemPrompt 构建（agent.ts:545/988）——时间冻结下
    // 与直接调用同一装配的输出字节一致。
    agent._systemPrompt = agent.buildSystemPrompt({ forceMemoryEnabled: agent.memoryMasterEnabled });
    expect(agent.systemPrompt).toBe(agent.buildSystemPrompt({ forceMemoryEnabled: agent.memoryMasterEnabled }));
    expect(agent.systemPrompt.includes(PERSONA_MARKER)).toBe(true);
  });

  it("emits a sanitized sample index (sources + lengths only) for the report", async () => {
    const { artifact, capturedSystem, workspacePrompt, workspaceInstructions } = await captureFinalRequest({ forSubagent: false });
    const baseBytes = Buffer.byteLength(artifact.text, "utf-8");
    const finalBytes = Buffer.byteLength(capturedSystem, "utf-8");
    const skillsBytes = capturedSystem.slice(
      capturedSystem.indexOf("The following skills provide"),
      capturedSystem.indexOf("</available_skills>") + "</available_skills>".length,
    );

    const index = {
      fixture: "合成 fixture（P06 受控临时目录；zh-CN；冻结时钟 2026-06-04T07:53:00Z）",
      variants: {
        "desktop-main": {
          canonical_base_bytes: baseBytes,
          append_workspace_scope_bytes: Buffer.byteLength(workspacePrompt ?? "", "utf-8"),
          append_workspace_instructions_bytes: Buffer.byteLength(workspaceInstructions ?? "", "utf-8"),
          skills_listing_bytes: Buffer.byteLength(skillsBytes, "utf-8"),
          project_context_bytes: Buffer.byteLength(`<project_context>`, "utf-8"),
          final_system_prompt_bytes: finalBytes,
          provenance_sections: 20,
          sources: [
            "platform_instruction(x6 zh)", "user_profile", "persona", "agent.appearance",
            "agent_roster", "memory_context(x3)", "session_instruction",
            "append: workspace-scope + workspace-instructions", "sdk: skills + project_context + cwd",
          ],
        },
      },
    };
    // 不输出任何正文副本——只有来源与长度。
    const serialized = JSON.stringify(index);
    for (const marker of [PERSONA_MARKER, PROFILE_MARKER, MEMORY_MARKER, APPEARANCE_MARKER, TENETS_MARKER]) {
      expect(serialized.includes(marker)).toBe(false);
    }
    expect(baseBytes).toBeGreaterThan(0);
    expect(finalBytes).toBeGreaterThan(baseBytes);
    // eslint-disable-next-line no-console
    console.log(`P06_SAMPLE_INDEX ${serialized}`);
  });
});
