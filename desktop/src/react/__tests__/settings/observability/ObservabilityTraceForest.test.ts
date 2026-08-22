/**
 * @vitest-environment jsdom
 *
 * Phase 9 buildTraceForest 测试 — roots/edges/orphanEdges 三事实构建、
 * visited-set 环截断（§九十二）、孤儿 Missing parent 合成节点（§九十）、
 * 未覆盖 call 防御（绝不静默丢行）。
 */
import { describe, expect, it } from 'vitest';
import type {
  ModelObservabilityCallListItem,
  ModelObservabilityTraceDetail,
  ModelObservabilityTraceListItem,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { buildTraceForest } from '../../../settings/tabs/observability/ObservabilityTraceExplorer';

function makeCall(callId: string): ModelObservabilityCallListItem {
  return {
    callId,
    traceId: 'tr_1',
    parentCallId: null,
    startedAt: '2026-08-22T08:00:00.000Z',
    endedAt: null,
    durationMs: null,
    terminalStatus: 'ok',
    persistenceCompleteness: 'complete',
    interruptedByRestart: false,
    model: { provider: 'openai', modelId: 'gpt-5', api: 'responses' },
    source: { subsystem: 'chat', operation: 'llm_call', surface: null, trigger: null },
    attribution: {
      kind: 'agent', sessionId: null, sessionPath: null, conversationId: null,
      conversationType: null, agentId: null, childAgentId: null, childSessionId: null, taskId: null,
    },
    callPurpose: 'chat',
    inputShape: 'chat_context',
    provenancePrecision: 'exact',
    provenance: { sectionCount: 3, opaqueCount: 0, categories: ['persona'] },
    payloadAvailability: 'present',
    payloadRecordCount: 1,
    usage: { availability: 'present', status: 'ok', summary: null },
    attemptCount: 1,
    providerRequestCount: 1,
  };
}

function makeDetail(
  calls: ModelObservabilityCallListItem[],
  roots: Array<{ callId: string; orphanParent: boolean }>,
  edges: Array<{ parentCallId: string; childCallId: string }>,
  orphanEdges: Array<{ childCallId: string; missingParentCallId: string }> = [],
): ModelObservabilityTraceDetail {
  const trace: ModelObservabilityTraceListItem = {
    traceId: 'tr_1', origin: null, firstSeenAt: '2026-08-22T08:00:00.000Z',
    lastSeenAt: '2026-08-22T08:00:00.000Z', callCount: calls.length,
    terminalOk: calls.length, terminalError: 0, terminalAborted: 0, incomplete: 0,
  };
  return {
    trace,
    calls,
    roots,
    edges,
    orphanEdges,
    graphIntegrity: 'ok',
    usageAggregate: {
      availability: 'present',
      summary: {
        inputTokens: 0, outputTokens: 0, reasoningTokens: 0, cacheReadTokens: 0,
        cacheWriteTokens: 0, totalTokens: 0, costTotal: null,
      },
    },
    payloadCompleteness: { present: 0, expired: 0, dropped: 0, notCaptured: 0, unknown: 0 },
    dataCompleteness: { droppedTraceEvents: 0, droppedPayloadRecords: 0, droppedBlobs: 0, interruptedByRestartCalls: 0 },
  };
}

describe('buildTraceForest (§八十七～九十五)', () => {
  it('builds nested children from edges, not array order (§八十九)', () => {
    const forest = buildTraceForest(makeDetail(
      [makeCall('a'), makeCall('b'), makeCall('c')],
      [{ callId: 'a', orphanParent: false }],
      [{ parentCallId: 'a', childCallId: 'b' }, { parentCallId: 'b', childCallId: 'c' }],
    ));
    expect(forest).toHaveLength(1);
    expect(forest[0].callId).toBe('a');
    expect(forest[0].children[0].callId).toBe('b');
    expect(forest[0].children[0].children[0].callId).toBe('c');
    expect(forest.every((n) => !n.cycle && !n.missingParent)).toBe(true);
  });

  it('multiple roots stay siblings', () => {
    const forest = buildTraceForest(makeDetail(
      [makeCall('a'), makeCall('b')],
      [{ callId: 'a', orphanParent: false }, { callId: 'b', orphanParent: false }],
      [],
    ));
    expect(forest.map((n) => n.callId)).toEqual(['a', 'b']);
  });

  it('orphan edges produce a synthetic Missing parent node (§九十)', () => {
    const forest = buildTraceForest(makeDetail(
      [makeCall('child')],
      [],
      [],
      [{ childCallId: 'child', missingParentCallId: 'ghost' }],
    ));
    expect(forest).toHaveLength(1);
    const ghost = forest[0];
    expect(ghost.callId).toBe('ghost');
    expect(ghost.missingParent).toBe(true);
    expect(ghost.call).toBeNull();
    expect(ghost.children[0].callId).toBe('child');
    expect(ghost.children[0].missingParent).toBe(false);
  });

  it('orphan child that is already a root is not duplicated', () => {
    const forest = buildTraceForest(makeDetail(
      [makeCall('a')],
      [{ callId: 'a', orphanParent: true }],
      [],
      [{ childCallId: 'a', missingParentCallId: 'ghost' }],
    ));
    expect(forest).toHaveLength(1);
    expect(forest[0].callId).toBe('a');
    expect(forest[0].missingParent).toBe(true);
  });

  it('cycles are truncated with a cycle flag instead of infinite recursion (§九十二)', () => {
    // a → b → a 环：roots 只给 a，递归 visited 必须截断。
    const forest = buildTraceForest(makeDetail(
      [makeCall('a'), makeCall('b')],
      [{ callId: 'a', orphanParent: false }],
      [{ parentCallId: 'a', childCallId: 'b' }, { parentCallId: 'b', childCallId: 'a' }],
    ));
    const b = forest[0].children[0];
    expect(b.callId).toBe('b');
    const cycleNode = b.children[0];
    expect(cycleNode.callId).toBe('a');
    expect(cycleNode.cycle).toBe(true);
    expect(cycleNode.children).toEqual([]);
  });

  it('calls not covered by roots/edges are surfaced as extra roots, never dropped', () => {
    const forest = buildTraceForest(makeDetail(
      [makeCall('a'), makeCall('lonely')],
      [{ callId: 'a', orphanParent: false }],
      [],
    ));
    expect(forest.map((n) => n.callId).sort()).toEqual(['a', 'lonely']);
  });
});
