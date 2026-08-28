/**
 * Phase 9 wire contract 锁定测试 — shared/model-observability-api-contract.ts
 * 是 renderer 唯一的 wire 事实源；闭集值一旦漂移，UI 标签/过滤映射会静默
 * 错位，这里把 UI 依赖的每个数组按值锁死。
 */
import { describe, expect, it } from 'vitest';
import {
  MODEL_CALL_PAYLOAD_FIDELITY,
  MODEL_CALL_PAYLOAD_KINDS,
  MODEL_CALL_PAYLOAD_SANITIZATION_STATUS,
  MODEL_CALL_PAYLOAD_VISIBILITY,
  MODEL_OBSERVABILITY_BLOB_ID_PATTERN,
  MODEL_OBSERVABILITY_BLOB_SAFE_MEDIA_MAJOR,
  MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS,
  MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS,
  MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT,
  MODEL_OBSERVABILITY_PAGE_MAX_LIMIT,
  MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES,
  MODEL_OBSERVABILITY_PAYLOAD_CONTENT_STATES,
  MODEL_OBSERVABILITY_TERMINAL_STATUSES,
  MODEL_OBSERVABILITY_USAGE_AVAILABILITIES,
  PROVIDER_MAPPING_PRECISION,
  PROVIDER_REQUEST_TRANSFORMATIONS,
  SEMANTIC_INPUT_CATEGORIES,
  SEMANTIC_INPUT_ROLES,
  SEMANTIC_INPUT_ROOTS,
  SEMANTIC_INPUT_SHAPES,
  SEMANTIC_SOURCE_TYPES,
} from '../../../../../../shared/model-observability-api-contract.ts';

describe('Model Observatory wire contract closed sets (Phase 9)', () => {
  it('locks terminal status / availability sets', () => {
    expect([...MODEL_OBSERVABILITY_TERMINAL_STATUSES]).toEqual(['ok', 'error', 'aborted', 'incomplete']);
    expect([...MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES]).toEqual([
      'present', 'expired', 'dropped', 'not_captured', 'unknown',
    ]);
    expect([...MODEL_OBSERVABILITY_USAGE_AVAILABILITIES]).toEqual([
      'present', 'corrupt', 'not_correlated', 'projection_unavailable', 'unknown',
    ]);
    expect([...MODEL_OBSERVABILITY_PAYLOAD_CONTENT_STATES]).toEqual([
      'present', 'null_payload', 'opaque_or_unavailable', 'corrupt',
    ]);
  });

  it('locks payload layer closed sets (kind/visibility/fidelity/sanitization)', () => {
    expect([...MODEL_CALL_PAYLOAD_KINDS]).toEqual([
      'semantic_request', 'provider_request', 'provider_response', 'semantic_response',
    ]);
    expect([...MODEL_CALL_PAYLOAD_VISIBILITY]).toEqual([
      'full', 'partial', 'metadata_only', 'opaque', 'unavailable',
    ]);
    expect([...MODEL_CALL_PAYLOAD_FIDELITY]).toEqual([
      'runtime_exact', 'parsed_equivalent', 'stream_aggregate', 'normalized',
      'metadata_only', 'external_process', 'opaque',
    ]);
    expect([...MODEL_CALL_PAYLOAD_SANITIZATION_STATUS]).toEqual([
      'none', 'redacted', 'truncated', 'degraded',
      'redacted_truncated', 'redacted_degraded', 'truncated_degraded', 'redacted_truncated_degraded',
    ]);
  });

  it('locks provenance / provider mapping closed sets', () => {
    expect([...PROVIDER_REQUEST_TRANSFORMATIONS]).toEqual([
      'pass_through', 'renamed', 'moved', 'merged', 'split',
      'filtered', 'injected', 'externalized', 'dropped', 'opaque',
    ]);
    expect([...PROVIDER_MAPPING_PRECISION]).toEqual(['exact', 'structural', 'opaque']);
    expect([...SEMANTIC_INPUT_CATEGORIES]).toHaveLength(24);
    expect(SEMANTIC_INPUT_CATEGORIES).toContain('unknown');
    expect([...SEMANTIC_INPUT_ROLES]).toHaveLength(7);
    expect([...SEMANTIC_SOURCE_TYPES]).toHaveLength(9);
    expect([...SEMANTIC_INPUT_ROOTS]).toEqual(['systemPrompt', 'messages', 'tools', 'input', 'parameters']);
    expect([...SEMANTIC_INPUT_SHAPES]).toHaveLength(9);
  });

  it('locks pagination / group-by limits', () => {
    expect(MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT).toBe(50);
    expect(MODEL_OBSERVABILITY_PAGE_MAX_LIMIT).toBe(200);
    expect(MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS).toBe(3);
    expect(MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS).toHaveLength(14);
    expect(MODEL_OBSERVABILITY_GROUP_BY_DIMENSIONS[0]).toBe('date');
  });

  it('blob id pattern accepts well-formed ids and rejects traversal', () => {
    expect(MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test('mb_abcd1234')).toBe(true);
    expect(MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test('mb_' + 'x'.repeat(96))).toBe(true);
    // 路径注入 / 非 mb_ 前缀 / 过长 / 非法字符全部拒绝。
    expect(MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test('mb_../etc/passwd')).toBe(false);
    expect(MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test('../etc/passwd')).toBe(false);
    expect(MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test('mb_' + 'x'.repeat(97))).toBe(false);
    expect(MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test('mb_a b')).toBe(false);
    expect(MODEL_OBSERVABILITY_BLOB_ID_PATTERN.test('MB_abcd')).toBe(false);
  });

  it('blob safe media majors are image/audio/video only', () => {
    expect([...MODEL_OBSERVABILITY_BLOB_SAFE_MEDIA_MAJOR]).toEqual(['image', 'audio', 'video']);
  });
});
