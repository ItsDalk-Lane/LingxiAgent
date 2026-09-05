/**
 * TraceDetailOverlay 渲染级测试 — 复现「详情页应呈现哪些记录行」的验收面。
 *
 * 用真实形状的会话消息 + 双调用 trace 渲染整层，断言三类记录行都在：
 * USER（用户消息）/ ASSISTANT（助手正文）/ TOOL（工具调用与结果），
 * 以及 SYSTEM 首记录与请求边界控件的存在性。
 */
// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, waitFor } from '@testing-library/react';
import React from 'react';
import type {
  ModelObservabilityCallListItem,
  ModelObservabilityTraceDetail,
  ModelObservabilityTracePage,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { useSettingsStore } from '../../../settings/store';
import { TraceDetailOverlay } from '../../../settings/tabs/observability/trace-detail/TraceDetailOverlay';

const T0 = Date.UTC(2026, 8, 1, 10, 0, 0);
const iso = (ms: number) => new Date(ms).toISOString();

const BASE_ATTRIBUTION = {
  kind: 'session', sessionId: 's-render', sessionPath: null, conversationId: 'c1',
  conversationType: 'dm', agentId: 'a1', childAgentId: null, childSessionId: null, taskId: null,
};

function makeCall(overrides: Partial<ModelObservabilityCallListItem> & { callId: string }): ModelObservabilityCallListItem {
  const { attribution, ...rest } = overrides;
  return {
    traceId: 'mt_render', parentCallId: null,
    startedAt: iso(T0), endedAt: iso(T0 + 2_000), durationMs: 2_000,
    terminalStatus: 'ok', persistenceCompleteness: 'known', interruptedByRestart: false,
    model: { provider: 'openai', modelId: 'gpt-test', api: 'responses' },
    source: { subsystem: 'llm', operation: 'chat', surface: 'server', trigger: 'user_turn' },
    attribution: { ...BASE_ATTRIBUTION, ...(attribution ?? {}) },
    callPurpose: null, inputShape: 'messages', provenancePrecision: 'exact',
    firstResponseAt: null,
    provenance: { sectionCount: 0, opaqueCount: 0, categories: [], categoriesState: 'absent' },
    payloadAvailability: 'not_captured', payloadRecordCount: 0,
    usage: { availability: 'present', status: 'ok', summary: null },
    attemptCount: 1, providerRequestCount: 1,
    ...rest,
  };
}

const payloadRecords = [
  {
    id: 9001, callId: 'main', kind: 'semantic_request', attemptId: 'att_1',
    providerRequestOrdinal: 1, capturedAt: iso(T0), visibility: 'full',
    fidelity: 'normalized', sanitizationStatus: 'redacted', redacted: true,
    truncated: false, degraded: false, recordCharCount: 2600, hasBody: true,
    hasSemanticProvenance: true, hasProviderProvenance: false, blobIds: [],
  },
  {
    id: 9002, callId: 'main', kind: 'semantic_response', attemptId: 'att_1',
    providerRequestOrdinal: 1, capturedAt: iso(T0), visibility: 'full',
    fidelity: 'normalized', sanitizationStatus: 'redacted', redacted: true,
    truncated: false, degraded: false, recordCharCount: 40, hasBody: true,
    hasSemanticProvenance: false, hasProviderProvenance: false, blobIds: [],
  },
];

const payloadBodies: Record<number, unknown> = {
  9001: {
    systemPrompt: `你是灵犀，有记忆的个人 AI 助手。\n保持简洁。\n\n${'附加行为准则段落。'.repeat(220)}`,
    messages: [
      { role: 'user', content: '帮我总结这个项目' },
    ],
  },
  9002: {
    completeness: 'complete',
    text: '这个项目分为三层。\n\n第二段正文。',
    finishReason: 'stop',
  },
};

const detail: ModelObservabilityTraceDetail = {
  trace: {
    traceId: 'mt_render', origin: 'user_turn', firstSeenAt: iso(T0), lastSeenAt: iso(T0 + 8_000),
    callCount: 2, terminalOk: 2, terminalError: 0, terminalAborted: 0, incomplete: 0,
  },
  calls: [
    makeCall({ callId: 'main', payloadAvailability: 'present', payloadRecordCount: 2 }),
    makeCall({
      callId: 'title', callPurpose: 'title',
      startedAt: iso(T0 + 6_000), endedAt: iso(T0 + 6_500), durationMs: 500,
    }),
  ],
  roots: [], edges: [], orphanEdges: [], graphIntegrity: 'ok',
  usageAggregate: {
    availability: 'complete', coveredCalls: 0, corruptCalls: 0,
    notCorrelatedCalls: 0, unknownCalls: 0, totalCalls: 2, summary: null,
  },
  payloadCompleteness: { present: 2, expired: 0, dropped: 0, notCaptured: 0, unknown: 0 },
  dataCompleteness: {
    status: 'known', droppedTraceEvents: 0, droppedPayloadRecords: 0,
    droppedBlobs: 0, interruptedByRestartCalls: 0,
  },
};

const sessionMessages = [
  {
    id: '1', role: 'user', content: '帮我总结这个项目',
    timestamp: iso(T0 - 5_000),
  },
  {
    id: '2', role: 'assistant', content: '这个项目分为三层。',
    timestamp: iso(T0 + 3_000),
    toolCalls: [
      {
        id: 'tu1', name: 'read_file', args: '{"path":"README.md"}',
        status: 'ok', success: true, details: { output: '# Lingxi\n项目根目录文件' },
      },
    ],
  },
];

const tracePage: ModelObservabilityTracePage = { traces: [], nextCursor: null };

function installFetchStub() {
  vi.stubGlobal('fetch', async (input: RequestInfo | URL) => {
    const raw = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const path = raw.replace(/^https?:\/\/[^/]+/, '');
    const json = (body: unknown) => new Response(JSON.stringify(body), {
      status: 200, headers: { 'content-type': 'application/json' },
    });
    if (/\/api\/model-observability\/calls\/[^/]+$/.test(path)) {
      return json({
        call: { ...detail.calls[0] },
        trace: { traceId: 'mt_render', origin: 'user_turn', firstSeenAt: iso(T0), lastSeenAt: iso(T0 + 8_000) },
        parentCall: null,
        childCalls: [],
        attempts: [],
        payloadRecords,
      });
    }
    if (/\/api\/model-observability\/payloads\/\d+$/.test(path)) {
      const id = Number(path.split('/').pop());
      const record = payloadRecords.find(candidate => candidate.id === id);
      return json({
        ...record,
        contentAvailable: true,
        contentState: 'present',
        payload: payloadBodies[id] ?? null,
      });
    }
    if (path.startsWith('/api/model-observability/traces/')) {
      return json({ ...detail, payloadRecords });
    }
    if (path.startsWith('/api/sessions/messages')) {
      return json({ messages: sessionMessages, hasMore: false, revision: '1:1' });
    }
    if (path.startsWith('/api/sessions/prompt-snapshot')) {
      return json({
        promptSnapshot: {
          version: 1,
          systemPrompt: '你是灵犀，有记忆的个人 AI 助手。',
          appendSystemPrompt: [],
          skillsResult: { skills: [], diagnostics: [] },
          agentsFilesResult: { agentsFiles: [] },
          finalSystemPrompt: '你是灵犀（最终组装版）。',
        },
        toolNames: ['read_file', 'write_file'],
      });
    }
    if (path.startsWith('/api/model-observability/query/traces')) return json(tracePage);
    throw new Error(`unexpected fetch: ${path}`);
  });
}

describe('TraceDetailOverlay 渲染验收', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('呈现 USER / ASSISTANT / TOOL 三类记录行 + SYSTEM 首记录', async () => {
    installFetchStub();
    (window as unknown as { t: unknown }).t = (key: string) => key;
    useSettingsStore.setState({
      serverPort: 1, serverToken: 'token', ready: true,
    } as never);

    render(<TraceDetailOverlay traceId="mt_render" onClose={() => {}} />);

    await waitFor(() => {
      const rows = document.querySelectorAll('tr[data-record-index]');
      expect(rows.length).toBeGreaterThan(0);
    });

    const kinds = [...document.querySelectorAll('tr[data-kind]')]
      .map(row => row.getAttribute('data-kind'));
    // 用户消息行 + 助手正文行 + 工具调用行都必须在场。
    expect(kinds).toContain('user');
    expect(kinds).toContain('message');
    expect(kinds).toContain('tool');

    // 用户与助手正文、工具名与结果在台账行文本中可见。
    const bodyText = document.body.textContent ?? '';
    expect(bodyText).toContain('帮我总结这个项目');
    expect(bodyText).toContain('这个项目分为三层。');
    expect(bodyText).toContain('read_file');

    // SYSTEM 首记录（dsh 首屏对齐）在场。
    expect(kinds).toContain('system');
  });

  it('SYSTEM 首记录正文来自会话提示词快照（无需开载荷捕获）', async () => {
    installFetchStub();
    (window as unknown as { t: unknown }).t = (key: string) => key;
    useSettingsStore.setState({
      serverPort: 1, serverToken: 'token', ready: true,
    } as never);

    render(<TraceDetailOverlay traceId="mt_render" onClose={() => {}} />);

    await waitFor(() => {
      expect(document.querySelectorAll('tr[data-record-index]').length).toBeGreaterThan(0);
    });
    // 打开 SYSTEM 首记录 → System Prompt tab 立即显示快照正文。
    const systemRow = document.querySelector('tr[data-kind="system"]');
    expect(systemRow).not.toBeNull();
    systemRow!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await waitFor(() => {
      const tabs = [...document.querySelectorAll('[role="tab"]')];
      const promptTab = tabs.find(tab => tab.textContent?.includes('settings.observability.traceDetail.tabs.systemPrompt'));
      expect(promptTab).toBeTruthy();
      promptTab!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    await waitFor(() => {
      expect(document.body.textContent).toContain('你是灵犀（最终组装版）。');
    });
  });

  it('观测载荷 tab：打开即直出 TXT 阅读式正文，超长截断可展开', async () => {
    installFetchStub();
    (window as unknown as { t: unknown }).t = (key: string) => key;
    useSettingsStore.setState({
      serverPort: 1, serverToken: 'token', ready: true,
    } as never);

    render(<TraceDetailOverlay traceId="mt_render" onClose={() => {}} />);

    // 打开助手正文记录（第一个 message 行）。
    await waitFor(() => {
      expect(document.querySelectorAll('tr[data-record-index]').length).toBeGreaterThan(0);
    });
    const messageRow = document.querySelector('tr[data-kind="message"]');
    expect(messageRow).not.toBeNull();
    messageRow!.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    // 切到「观测载荷」tab。
    await waitFor(() => {
      const tabs = [...document.querySelectorAll('[role="tab"]')];
      const payloadsTab = tabs.find(tab => tab.textContent?.includes('settings.observability.traceDetail.tabs.payloads'));
      expect(payloadsTab).toBeTruthy();
      payloadsTab!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });

    // 正文直出（无 loadBody 按钮、无 JSON 语法呈现）：语义请求的 systemPrompt
    // 与消息、语义响应的输出文本都以自然分段文本出现。
    await waitFor(() => {
      const body = document.body.textContent ?? '';
      expect(body).toContain('你是灵犀，有记忆的个人 AI 助手。');
      expect(body).toContain('帮我总结这个项目');
      expect(body).toContain('这个项目分为三层。');
    });
    // 不再出现懒加载按钮文案。
    expect(document.body.textContent).not.toContain('settings.observability.payload.loadBody');
    // 超长块（2600ch）有「显示更多」展开控件。
    const moreButtons = [...document.querySelectorAll('button')].filter(
      button => button.textContent?.includes('settings.observability.traceDetail.payloadView.more'),
    );
    expect(moreButtons.length).toBeGreaterThan(0);
  });
});
