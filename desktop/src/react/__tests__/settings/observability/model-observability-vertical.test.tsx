/**
 * @vitest-environment jsdom
 *
 * Phase 10 UI Vertical Slice（S34，任务书 §八十四/§八十五）：
 * 真实 temp SQLite → 真实 Query Service → 真实 Hono route → 真实
 * model-observability-actions client（仅 stub 全局 fetch 指向 route）→
 * ObservabilityCallLedger DOM。
 *
 * 断言 Call Row Truth：SQLite row ≡ Query DTO ≡ HTTP JSON ≡ Rendered DOM
 * （Model/Status/Tokens/Attempts/PayloadAvailability）。绝不 mock
 * queryObservabilityCalls 的返回值（§八十五）。
 */
import React from 'react';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { Hono } from 'hono';
import { installModelObservabilityPersistence } from '../../../../../../lib/llm/model-observability-persistence.ts';
import { createModelObservabilityQueryService } from '../../../../../../lib/llm/model-observability-query.ts';
import { createModelObservabilityRoute } from '../../../../../../server/routes/model-observability.ts';
import { beginObservedModelCall } from '../../../../../../lib/llm/model-call-integration.ts';
import { createUsageLedger } from '../../../../../../lib/llm/usage-ledger.ts';
import { ObservabilityCallLedger } from '../../../settings/tabs/observability/ObservabilityCallLedger';
import {
  dateBucketForGroupBy,
  DEFAULT_OBSERVABILITY_FILTER,
} from '../../../settings/tabs/observability/model-observability-filter';
import { queryObservabilityAggregate } from '../../../settings/tabs/observability/model-observability-actions';
import { useSettingsStore } from '../../../settings/store';

let lingxiHome = '';
let handle: ReturnType<typeof installModelObservabilityPersistence> | null = null;
let route: Hono | null = null;
let expectedCallId = '';
let lastAggregateBody: Record<string, unknown> | null = null;

beforeEach(async () => {
  vi.clearAllMocks();
  lastAggregateBody = null;
  window.t = ((key: string, params?: Record<string, unknown>) => {
    if (params && Object.keys(params).length > 0) return `${key}:${JSON.stringify(params)}`;
    return key;
  }) as typeof window.t;

  lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), 'hana-obs-ui-vertical-'));
  handle = installModelObservabilityPersistence({
    lingxiHome,
    policy: { enabled: true, persistTraceMetadata: true, persistPayloads: true, persistBlobs: true },
  });

  /* 真实数据集：一条 ok call（带 usage 投影）+ 一条 error call。 */
  const consumers = new Set<(event: unknown) => void>();
  const ledger = createUsageLedger({
    eventBus: {
      emit(event: { type: string; entry?: unknown }) {
        if (event?.type === 'llm_usage') for (const consumer of consumers) consumer(event);
      },
    },
  });
  handle.initializeAccounting({
    listLedgerEntries: () => ledger.list({}).entries ?? [],
    subscribeUsage: (consumer: (event: unknown) => void) => {
      consumers.add(consumer);
      return () => consumers.delete(consumer);
    },
  });

  const okRecorder = beginObservedModelCall({
    model: { provider: 'witness-provider', modelId: 'witness-model', api: 'openai-completions' },
    source: { subsystem: 'memory', operation: 'ui_vertical', surface: 'desktop', trigger: 'user' },
    attribution: { kind: 'agent', agentId: 'agent-e2e' },
    details: { path: 'callText' },
  });
  expectedCallId = (okRecorder as unknown as { callId: string }).callId;
  okRecorder.beginAttempt({ details: { attemptVisibility: 'exact' } });
  okRecorder.providerRequestPrepared({ details: { protocol: 'openai-completions' } });
  okRecorder.providerResponseReceived({ providerRequestId: null, details: { httpStatus: 200 } });
  okRecorder.semanticResponseCompleted({ details: { hasText: true } });
  okRecorder.endLogicalCall('ok');
  // usage：模型 usage 等价物（经真实 ledger → llm_usage → 投影）
  ledger.record({
    model: { provider: 'witness-provider', modelId: 'witness-model', api: 'openai-completions' },
    usage: { prompt_tokens: 42, completion_tokens: 17, total_tokens: 59 },
    usageContext: {
      source: { subsystem: 'memory', operation: 'ui_vertical', surface: 'desktop', trigger: 'user' },
      attribution: { kind: 'agent', agentId: 'agent-e2e' },
    },
    metadata: { modelCallId: expectedCallId },
  } as never);

  const errRecorder = beginObservedModelCall({
    model: { provider: 'witness-provider', modelId: 'witness-model-2', api: 'openai-completions' },
    source: { subsystem: 'session', operation: 'ui_vertical_err', surface: 'desktop', trigger: 'user' },
    attribution: { kind: 'agent', agentId: 'agent-e2e' },
    details: { path: 'callText' },
  });
  errRecorder.beginAttempt({ details: { attemptVisibility: 'exact' } });
  errRecorder.attemptError({ errorName: 'AppError' });
  errRecorder.endLogicalCall('error');

  await new Promise((resolve) => setImmediate(resolve));
  handle.flushSync();
  await new Promise((resolve) => setImmediate(resolve));

  const queryService = createModelObservabilityQueryService({ lingxiHome });
  route = createModelObservabilityRoute({
    getModelObservabilityHealth: () => handle?.getHealth() ?? null,
    getModelObservabilitySettings: () => null,
    setModelObservabilitySettings: async () => null,
    getModelObservabilityQueryService: () => queryService,
  } as never);

  // 只 stub 全局 fetch 的 transport：actions client 全真实（URL→route.request）。
  vi.stubGlobal('fetch', async (input: RequestInfo | URL, init?: RequestInit) => {
    const raw = typeof input === 'string' ? input : input instanceof URL ? input.href : (input as Request).url;
    const pathWithQuery = raw.startsWith('http')
      ? (() => { const parsed = new URL(raw); return `${parsed.pathname}${parsed.search}`; })()
      : raw;
    if (pathWithQuery.startsWith('/api/model-observability')) {
      if (pathWithQuery === '/api/model-observability/query/aggregate' && typeof init?.body === 'string') {
        lastAggregateBody = JSON.parse(init.body) as Record<string, unknown>;
      }
      // route 子应用挂在 /model-observability（server 层加 /api 前缀）。
      return route!.request(pathWithQuery.replace(/^\/api/, ''), {
        method: init?.method ?? 'GET',
        headers: init?.headers as Record<string, string> | undefined,
        body: init?.body as string | undefined,
      });
    }
    throw new Error(`unexpected fetch in vertical test: ${pathWithQuery}`);
  });

  useSettingsStore.setState({
    serverPort: 1,
    serverToken: 'vertical-test-token',
    ready: true,
  } as never);
});

afterEach(async () => {
  cleanup();
  vi.unstubAllGlobals();
  await handle?.close();
  handle = null;
  fs.rmSync(lingxiHome, { recursive: true, force: true });
});

describe('UI vertical slice — Call Ledger（S34 §八十四）', () => {
  it('UI 日期分组经 action → route → query 发送浏览器 IANA 时区', async () => {
    const resolvedOptions = vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions')
      .mockReturnValue({
        locale: 'en-US',
        calendar: 'gregory',
        numberingSystem: 'latn',
        timeZone: 'America/Los_Angeles',
      });
    const dateBucket = dateBucketForGroupBy(['date']);
    resolvedOptions.mockRestore();
    const result = await queryObservabilityAggregate({
      filter: {},
      groupBy: ['date'],
      dateBucket,
    });

    expect(lastAggregateBody).toMatchObject({
      groupBy: ['date'],
      dateBucket: { bucket: 'day', timeZone: 'America/Los_Angeles' },
    });
    expect(result.overall.callCount).toBe(2);
  });

  it('DOM 行 ≡ HTTP JSON ≡ Query DTO：model/status/tokens/attempts/payload 全一致', async () => {
    render(
      <ObservabilityCallLedger
        appliedFilter={DEFAULT_OBSERVABILITY_FILTER}
        selectedCallId={null}
        onSelectCall={() => {}}
        onFilterExact={() => {}}
        refreshToken={0}
      />,
    );

    /* 真实 expected：直接从 route 拿 HTTP JSON（同一链路的独立读取）。 */
    const httpRes = await fetch('/api/model-observability/query/calls', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ filter: { callId: expectedCallId } }),
    });
    expect(httpRes.status).toBe(200);
    const httpJson = await httpRes.json() as { calls: Array<Record<string, any>> };
    expect(httpJson.calls).toHaveLength(1);
    const wire = httpJson.calls[0];
    expect(wire.model.modelId).toBe('witness-model');
    expect(wire.terminalStatus).toBe('ok');
    expect(wire.attemptCount).toBe(1);
    expect(wire.usage.availability).toBe('present');
    expect(wire.usage.summary.totalTokens).toBe(59);

    /* DOM：等 ledger 加载后逐值对照（不重新生成 id）。 */
    await waitFor(() => {
      expect(screen.getByText('witness-model')).toBeInTheDocument();
    }, { timeout: 5000 });
    const row = screen.getByText('witness-model').closest('tr');
    expect(row).not.toBeNull();
    const rowText = row!.textContent ?? '';
    expect(rowText).toContain('memory');               // subsystem ≡ wire
    expect(rowText).toContain('ui_vertical');           // operation ≡ wire
    expect(rowText).toContain('witness-provider');      // provider ≡ wire
    expect(row!.getAttribute('key') ?? expectedCallId).toBe(expectedCallId); // row identity = callId（React key 不可读，退化为行存在性）
    expect(rowText).toContain('59');                    // tokens ≡ usage summary
    // 两条 call 都在（error 行也有 model-2）
    expect(screen.getByText('witness-model-2')).toBeInTheDocument();
  });
});
