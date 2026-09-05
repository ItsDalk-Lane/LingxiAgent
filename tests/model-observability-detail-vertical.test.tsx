/**
 * @vitest-environment jsdom
 *
 * Phase 10.1 详情纵向验收：真实 callText → 持久化 → Query → Hono →
 * renderer action → Call Inspector / Payload / Trace Explorer。
 */
import React from 'react';
import '../desktop/src/assets.d.ts';
import { createAssistantMessageEventStream } from '@earendil-works/pi-ai';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { callText } from '../core/llm-client.ts';
import {
  installModelCallStreamObserver,
  installModelCallTraceIngress,
} from '../lib/pi-sdk/model-call-stream-observer.ts';
import { agentToolToToolDefinition } from '../lib/pi-sdk/session-options.ts';
import { runWithModelTraceRoot } from '../lib/llm/model-trace-scope.ts';
import { runCachePreservingCompactionAgentRun } from '../lib/llm/cache-preserving-compaction-agent-run.ts';
import {
  createScenarioHarness,
  flushAsync,
  openaiCompletionsJson,
  type ScenarioHarness,
} from './helpers/model-observability-scenario-harness.ts';
import { ObservabilityCallInspector } from '../desktop/src/react/settings/tabs/observability/ObservabilityCallInspector';
import {
  buildTraceForest,
  ObservabilityTraceExplorer,
} from '../desktop/src/react/settings/tabs/observability/ObservabilityTraceExplorer';
import { DEFAULT_OBSERVABILITY_FILTER } from '../desktop/src/react/settings/tabs/observability/model-observability-filter';
import { useSettingsStore } from '../desktop/src/react/settings/store';

const USER_INPUT = 'PHASE10_DETAIL_VERTICAL_USER_INPUT';
const PI_MODEL = { id: 'phase10-pi-model', provider: 'witness-provider', api: 'openai-completions' };

let harness: ScenarioHarness;
let ledger: ReturnType<ScenarioHarness['createLedger']>;
let callId = '';
let traceId = '';
let requestedPaths: string[] = [];

function assistantMessage(overrides: Record<string, unknown> = {}) {
  return {
    role: 'assistant',
    content: [{ type: 'text', text: 'ok' }],
    api: PI_MODEL.api,
    provider: PI_MODEL.provider,
    model: PI_MODEL.id,
    usage: { input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15 },
    stopReason: 'stop',
    timestamp: Date.now(),
    ...overrides,
  };
}

function streamOf(message: ReturnType<typeof assistantMessage>) {
  const stream = createAssistantMessageEventStream();
  queueMicrotask(() => {
    stream.push({ type: 'done', reason: message.stopReason, message } as never);
    stream.end();
  });
  return stream;
}

function fakeSession(streamFunction: (...args: any[]) => unknown, options: {
  sessionId?: string;
  sessionPath?: string;
  isCompacting?: boolean;
} = {}) {
  const session: any = {
    agent: { streamFunction },
    sessionManager: {
      getSessionId: () => options.sessionId ?? 'phase10-parent-session',
      getSessionFile: () => options.sessionPath ?? '/tmp/phase10-parent.jsonl',
    },
    isCompacting: options.isCompacting ?? false,
    async prompt(_text: string) {
      const stream = await session.agent.streamFunction(PI_MODEL, { messages: [] }, {});
      await stream.result();
    },
  };
  return session;
}

beforeEach(async () => {
  vi.clearAllMocks();
  requestedPaths = [];
  window.t = ((key: string, params?: Record<string, unknown>) => (
    params && Object.keys(params).length > 0 ? `${key}:${JSON.stringify(params)}` : key
  )) as typeof window.t;

  harness = await createScenarioHarness();
  ledger = harness.createLedger();
  harness.witness.scriptNext({
    kind: 'json',
    body: openaiCompletionsJson({
      content: 'PHASE10_DETAIL_VERTICAL_REPLY',
      usage: { prompt_tokens: 23, completion_tokens: 7, total_tokens: 30 },
    }),
  });

  await runWithModelTraceRoot({ origin: 'user_turn' }, async () => {
    await callText({
      api: 'openai-completions',
      apiKey: 'phase10-local-witness-key',
      baseUrl: harness.witness.baseUrl,
      model: { id: 'phase10-detail-model', provider: 'witness-provider' },
      systemPrompt: 'PHASE10_DETAIL_VERTICAL_SYSTEM',
      messages: [{ role: 'user', content: USER_INPUT }],
      usageLedger: ledger,
      usageContext: {
        source: { subsystem: 'memory', operation: 'detail_vertical', surface: 'desktop', trigger: 'user' },
        attribution: { kind: 'agent', agentId: 'agent-detail-vertical' },
      },
    } as never);
  });
  await flushAsync();
  harness.flush();
  await flushAsync();

  [callId] = harness.observer!.callIds();
  traceId = harness.observer!.callIdentity(callId)!.traceId!;
  const route = harness.route();
  vi.stubGlobal('fetch', async (input: RequestInfo | URL, init?: RequestInit) => {
    const raw = typeof input === 'string'
      ? input
      : input instanceof URL
        ? input.href
        : (input as Request).url;
    const pathWithQuery = raw.startsWith('http')
      ? (() => { const parsed = new URL(raw); return `${parsed.pathname}${parsed.search}`; })()
      : raw;
    if (!pathWithQuery.startsWith('/api/model-observability')) {
      throw new Error(`unexpected fetch in observability detail vertical: ${pathWithQuery}`);
    }
    requestedPaths.push(pathWithQuery);
    return route.request(pathWithQuery.replace(/^\/api/, ''), {
      method: init?.method ?? 'GET',
      headers: init?.headers as Record<string, string> | undefined,
      body: init?.body as string | undefined,
    });
  });

  useSettingsStore.setState({
    serverPort: 1,
    serverToken: 'detail-vertical-token',
    ready: true,
  } as never);
});

afterEach(async () => {
  cleanup();
  vi.unstubAllGlobals();
  await harness.close();
  harness.cleanup();
});

describe('Model Observatory 详情纵向链', () => {
  it('Call Inspector 经真实 HTTP detail 展示调用，并自动加载纯文本 Payload 正文', async () => {
    render(
      <ObservabilityCallInspector
        callId={callId}
        isLocalOwner
        onClose={() => {}}
        onOpenTrace={() => {}}
      />,
    );

    await waitFor(() => {
      expect(screen.getAllByText(/phase10-detail-model/).length).toBeGreaterThan(0);
    });
    expect(requestedPaths).toContain(`/api/model-observability/calls/${callId}`);

    await waitFor(() => {
      expect(screen.getAllByText(new RegExp(USER_INPUT)).length).toBeGreaterThan(0);
    });
    expect(screen.queryByText('settings.observability.payload.loadBody')).toBeNull();
    expect(requestedPaths.some((value) => /\/api\/model-observability\/payloads\/\d+$/.test(value))).toBe(true);
  });

  it('Trace Explorer 经真实 HTTP list/detail：单次调用的会话轨迹进入列表，详情层渲染调用记录', async () => {
    render(
      <ObservabilityTraceExplorer
        appliedFilter={DEFAULT_OBSERVABILITY_FILTER}
        selectedTraceId={traceId}
        onSelectTrace={() => {}}
        onSelectCall={() => {}}
        refreshToken={0}
      />,
    );

    // 轨迹按会话聚合（产品口径 2026-09-05）后不过滤单次调用（minCallCount=1）：
    // 本 fixture 只发了一次调用 → 列表仍出现这条会话轨迹。
    await waitFor(() => {
      expect(document.querySelector('[class*="observability-trace-row"]')).not.toBeNull();
    });
    expect(document.querySelector('[data-state="no-results"]')).toBeNull();
    expect(requestedPaths).toContain('/api/model-observability/query/traces');
    // 详情层（dsh ui-trajectory 布局）经真实 HTTP 拉取 trace 并渲染记录行。
    await waitFor(() => {
      expect(document.querySelector('tr[data-record-index]')).not.toBeNull();
    });
    expect(requestedPaths).toContain(`/api/model-observability/traces/${traceId}`);
  });

  it('并行工具的两个子调用经真实工具边界落入 Store、Query 和 Trace UI 同一棵树', async () => {
    let ordinal = 0;
    const session = fakeSession(async () => {
      ordinal += 1;
      return streamOf(assistantMessage(ordinal === 1 ? {
        content: [
          { type: 'toolCall', id: 'tc_parallel_a', name: 'parallel_a', arguments: {} },
          { type: 'toolCall', id: 'tc_parallel_b', name: 'parallel_b', arguments: {} },
        ],
      } : {}));
    });
    installModelCallStreamObserver(session);
    const toolA = agentToolToToolDefinition({
      name: 'parallel_a',
      execute: async () => (await session.agent.streamFunction(PI_MODEL, { messages: [] }, {})).result(),
    } as never);
    const toolB = agentToolToToolDefinition({
      name: 'parallel_b',
      execute: async () => {
        await new Promise((resolve) => setTimeout(resolve, 5));
        return (await session.agent.streamFunction(PI_MODEL, { messages: [] }, {})).result();
      },
    } as never);
    const before = new Set(harness.observer!.callIds());

    await runWithModelTraceRoot({ origin: 'user_turn' }, async () => {
      const first = await session.agent.streamFunction(PI_MODEL, { messages: [] }, {});
      const rootMessage = await first.result();
      expect(rootMessage.content).toHaveLength(2);
      await Promise.all([
        toolA.execute('tc_parallel_a', {}, undefined, undefined),
        toolB.execute('tc_parallel_b', {}, undefined, undefined),
      ]);
    });
    await flushAsync();
    harness.flush();
    await flushAsync();

    const [rootCallId, childA, childB] = harness.observer!.callIds().filter((id) => !before.has(id));
    const runtimeTraceId = harness.observer!.callIdentity(rootCallId)!.traceId!;
    const detailResult = harness.query().queryTraceDetail(runtimeTraceId);
    expect(detailResult.ok).toBe(true);
    if (detailResult.ok === false) throw new Error(detailResult.error.message);
    expect(detailResult.value.calls.map((call) => call.callId).sort())
      .toEqual([rootCallId, childA, childB].sort());
    expect(detailResult.value.edges).toEqual(expect.arrayContaining([
      { parentCallId: rootCallId, childCallId: childA },
      { parentCallId: rootCallId, childCallId: childB },
    ]));
    const forest = buildTraceForest(detailResult.value);
    expect(forest).toHaveLength(1);
    expect(forest[0].callId).toBe(rootCallId);
    expect(forest[0].children.map((node) => node.callId).sort()).toEqual([childA, childB].sort());
  });

  it('子代理跨会话调用经真实 spawn 工具边界落入 Store、Query 和 Trace UI 父子树', async () => {
    const parentSession = fakeSession(async () => streamOf(assistantMessage()));
    const childSession = fakeSession(
      async () => streamOf(assistantMessage()),
      { sessionId: 'phase10-child-session', sessionPath: '/tmp/phase10-child.jsonl' },
    );
    installModelCallStreamObserver(parentSession);
    installModelCallStreamObserver(childSession);
    installModelCallTraceIngress(childSession);
    const spawnTool = agentToolToToolDefinition({
      name: 'spawn_subagent',
      execute: async () => {
        await childSession.prompt('run child task');
        return { content: [] };
      },
    } as never);
    const before = new Set(harness.observer!.callIds());

    await runWithModelTraceRoot({ origin: 'user_turn' }, async () => {
      const first = await parentSession.agent.streamFunction(PI_MODEL, { messages: [] }, {});
      await first.result();
      await spawnTool.execute('tc_spawn', {}, undefined, undefined);
    });
    await flushAsync();
    harness.flush();
    await flushAsync();

    const [rootCallId, childCallId] = harness.observer!.callIds().filter((id) => !before.has(id));
    const runtimeTraceId = harness.observer!.callIdentity(rootCallId)!.traceId!;
    const detailResult = harness.query().queryTraceDetail(runtimeTraceId);
    expect(detailResult.ok).toBe(true);
    if (detailResult.ok === false) throw new Error(detailResult.error.message);
    expect(detailResult.value.edges).toContainEqual({ parentCallId: rootCallId, childCallId });
    expect(detailResult.value.calls.find((call) => call.callId === childCallId)?.attribution.sessionId)
      .toBe('phase10-child-session');
    const forest = buildTraceForest(detailResult.value);
    expect(forest[0].callId).toBe(rootCallId);
    expect(forest[0].children[0].callId).toBe(childCallId);
  });

  it('MC-02 AgentRun 的语义输入、响应、usage 和 unavailable provider wire 纵向一致', async () => {
    const validSummary = [
      '## Goal', '- g',
      '## Constraints & Preferences', '- c',
      '## Progress', '### Done', '- d', '### In Progress', '- i', '### Blocked', '- b',
      '## Key Decisions', '- k',
      '## Next Steps', '- n',
      '## Critical Context', '- x',
    ].join('\n');
    const session = fakeSession(async () => streamOf(assistantMessage({
      content: [{ type: 'text', text: validSummary }],
    })));
    installModelCallStreamObserver(session);
    const before = new Set(harness.observer!.callIds());

    const result = await runCachePreservingCompactionAgentRun({
      liveMessages: [{ role: 'user', content: [{ type: 'text', text: 'old context' }], timestamp: 1 }],
      instruction: {
        role: 'user',
        content: [{ type: 'text', text: 'PHASE10_MC02_COMPACTION_INSTRUCTION' }],
        timestamp: 2,
      },
      tools: [],
      model: { ...PI_MODEL, contextWindow: 128_000, maxTokens: 8_192 } as never,
      systemPrompt: 'PHASE10_MC02_SYSTEM',
      convertToLlm: async (messages: unknown) => messages,
      streamFn: session.agent.streamFunction,
      usageLedger: ledger,
      usageContext: {
        source: { subsystem: 'compaction', operation: 'compact', surface: 'desktop', trigger: 'threshold' },
        attribution: { kind: 'session', sessionId: 'phase10-parent-session' },
      },
      cacheMetadata: { cacheStrategy: 'session_snapshot', strict: true },
    } as never);
    expect(result.summary).toBe(validSummary);
    await flushAsync();
    harness.flush();
    await flushAsync();

    const [mc02CallId] = harness.observer!.callIds().filter((id) => !before.has(id));
    const detailResult = harness.query().queryCallDetail(mc02CallId);
    expect(detailResult.ok).toBe(true);
    if (detailResult.ok === false) throw new Error(detailResult.error.message);
    expect(detailResult.value.call.traceId).toBeTruthy();
    expect(detailResult.value.call.usage.availability).toBe('present');
    expect(detailResult.value.call.usage.summary?.totalTokens).toBe(15);
    expect(detailResult.value.payloadRecords.map((record) => [record.kind, record.visibility]))
      .toEqual(expect.arrayContaining([
        ['semantic_request', 'full'],
        ['provider_request', 'unavailable'],
        ['provider_response', 'unavailable'],
        ['semantic_response', 'full'],
      ]));
    const semanticRequestMeta = detailResult.value.payloadRecords.find((record) => record.kind === 'semantic_request')!;
    const semanticRequest = harness.query().getPayloadRecord(semanticRequestMeta.id);
    expect(semanticRequest.ok).toBe(true);
    if (semanticRequest.ok) {
      expect(JSON.stringify(semanticRequest.value.payload)).toContain('PHASE10_MC02_COMPACTION_INSTRUCTION');
    }
  });

  it('MC-03 native compaction 从真实 isCompacting 边界写入显式 not_correlated，不用缺行猜测', async () => {
    const session = fakeSession(
      async () => streamOf(assistantMessage({ content: [{ type: 'text', text: 'PHASE10_MC03_SUMMARY' }] })),
      { isCompacting: true },
    );
    installModelCallStreamObserver(session);
    const before = new Set(harness.observer!.callIds());

    const stream = await session.agent.streamFunction(PI_MODEL, {
      systemPrompt: 'PHASE10_MC03_SUMMARIZER_SYSTEM',
      messages: [{ role: 'user', content: [{ type: 'text', text: '<conversation>history</conversation>' }] }],
      tools: [],
    }, {});
    await stream.result();
    await flushAsync();
    harness.flush();
    await flushAsync();

    const [mc03CallId] = harness.observer!.callIds().filter((id) => !before.has(id));
    const detailResult = harness.query().queryCallDetail(mc03CallId);
    expect(detailResult.ok).toBe(true);
    if (detailResult.ok === false) throw new Error(detailResult.error.message);
    expect(detailResult.value.call.source).toMatchObject({ subsystem: 'compaction', operation: 'compact' });
    expect(detailResult.value.call.usage.availability).toBe('not_correlated');
    expect(detailResult.value.call.usage.summary).toBeNull();
    expect(detailResult.value.payloadRecords.map((record) => [record.kind, record.visibility]))
      .toEqual(expect.arrayContaining([
        ['semantic_request', 'full'],
        ['provider_request', 'unavailable'],
        ['provider_response', 'unavailable'],
        ['semantic_response', 'full'],
      ]));
  });
});
