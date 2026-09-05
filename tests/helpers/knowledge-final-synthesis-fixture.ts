import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { ModelRuntime, DefaultResourceLoader, SessionManager } from "@earendil-works/pi-coding-agent";
import { createAgentSession } from "../../lib/pi-sdk/index.ts";
import { createModelCallObserverExtension } from "../../lib/extensions/model-call-observer-ext.ts";
import { estimateTextTokens } from "../../lib/llm/estimate-text-tokens.ts";
import { createScenarioHarness, flushAsync, openaiCompletionsSseBody } from "./model-observability-scenario-harness.ts";

/** 真实最终会话经本地HTTP供应商替身返回答案；测量传输/观测链，不声称是在线模型性能。 */
export async function measureKnowledgeFinalSynthesis(block: string, partial: boolean) {
  const harness = await createScenarioHarness();
  let session: Awaited<ReturnType<typeof createAgentSession>>["session"] | undefined;
  try {
    const answer = partial ? "核查尚有缺口或冲突，只能陈述已检查范围中的资料事实。{{cite:1}}" : "已核对给定材料中的原文。{{cite:1}}";
    assert.ok(block.includes("[K1] EvidenceId:"));
    const usage = { prompt_tokens: estimateTextTokens(block), completion_tokens: estimateTextTokens(answer),
      total_tokens: estimateTextTokens(block) + estimateTextTokens(answer) };
    harness.witness.scriptNext({ kind: "sse", body: openaiCompletionsSseBody({ content: answer, usage }) });
    const runtime = await ModelRuntime.create({ modelsPath: null, refreshOnCreate: false });
    runtime.registerProvider("knowledge-benchmark", { name: "本地基准供应商", baseUrl: `${harness.witness.baseUrl}/v1`,
      api: "openai-completions", apiKey: "local-benchmark-placeholder", authHeader: true });
    const loader = new DefaultResourceLoader({ cwd: harness.lingxiHome, agentDir: harness.lingxiHome,
      noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
      extensionFactories: [createModelCallObserverExtension()] });
    await loader.reload();
    const created = await createAgentSession({ cwd: harness.lingxiHome, modelRuntime: runtime, resourceLoader: loader,
      sessionManager: SessionManager.inMemory(), noTools: "all", model: {
        id: "knowledge-benchmark", provider: "knowledge-benchmark", api: "openai-completions", baseUrl: `${harness.witness.baseUrl}/v1`,
        name: "本地基准模型", reasoning: false, input: ["text"], contextWindow: 32768, maxTokens: 1024,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      } });
    session = created.session;
    const started = performance.now(); await session.prompt(block); const durationMs = performance.now() - started;
    await flushAsync(5); harness.flush(); await flushAsync(3);
    const requests = harness.witness.requestsTo("/chat/completions");
    assert.equal(requests.length, 1);
    assert.ok(JSON.stringify(requests[0].bodyJson).includes("KnowledgeResearchContext"));
    const callIds = harness.observer!.callIds(); assert.equal(callIds.length, 1);
    const detail = harness.query().queryCallDetail(callIds[0]); assert.equal(detail.ok, true);
    if (!detail.ok) throw new Error("最终合成调用未持久化");
    assert.deepEqual(detail.value.payloadRecords.map(record => record.kind).sort(),
      ["provider_request", "provider_response", "semantic_request", "semantic_response"]);
    const assistant = session.messages.filter(message => message.role === "assistant").at(-1);
    assert.ok(assistant && assistant.role === "assistant");
    const text = assistant.content.filter(item => item.type === "text").map(item => item.text).join("");
    assert.equal(text, answer);
    assert.equal(assistant.usage.input, usage.prompt_tokens); assert.equal(assistant.usage.output, usage.completion_tokens);
    const citations = [...text.matchAll(/\{\{cite:(\d+)\}\}/gu)].map(match => Number(match[1]));
    assert.ok(citations.length > 0 && citations.every(id => block.includes(`[K${id}] EvidenceId:`)));
    return { durationMs, modelCalls: requests.length, observedCalls: callIds.length, citations,
      usage: { input: assistant.usage.input, output: assistant.usage.output, totalTokens: assistant.usage.totalTokens },
      boundary: "real AgentSession and HTTP/stream observer; local scripted provider; usage declared from fixture token estimates, not billed provider usage" };
  } finally { await session?.dispose(); await harness.close(); harness.cleanup(); }
}
