/**
 * Phase 9 Semantic Locator 解析测试 — locator-only 纪律（§七十二～七十六）。
 * UI 只按 root+path+UTF-16 span 从同一 payload 取值；结果三态必须诚实。
 */
import { describe, expect, it } from 'vitest';
import {
  asSemanticInputProvenance,
  resolveProviderLocator,
  resolveSemanticLocator,
} from '../../../settings/tabs/observability/observability-provenance-resolve';

const PAYLOAD = {
  systemPrompt: 'You are Hana. Stay concise.',
  messages: [
    { role: 'user', content: 'Hello world' },
    { role: 'assistant', content: [{ type: 'text', text: 'Hi! 👋' }] },
  ],
  tools: [{ name: 'search', description: 'Search the web' }],
  parameters: { temperature: 0.7 },
};

describe('resolveSemanticLocator', () => {
  it('resolves a span inside a nested string (UTF-16 slice semantics)', () => {
    // "Hello world"[0,5] === "Hello"
    const result = resolveSemanticLocator(PAYLOAD, {
      root: 'messages', path: [0, 'content'], span: { start: 0, end: 5 },
    });
    expect(result).toEqual({ status: 'resolved', text: 'Hello', containerPreview: undefined });
  });

  it('resolves identity-root string with a full span (emoji surrogate pair safe)', () => {
    const result = resolveSemanticLocator(PAYLOAD, {
      root: 'systemPrompt', path: undefined, span: { start: 0, end: 12 },
    });
    expect(result.status).toBe('resolved');
    if (result.status === 'resolved') expect(result.text).toBe('You are Hana');
  });

  it('null span → structural with honest value kind, never a body slice', () => {
    const result = resolveSemanticLocator(PAYLOAD, { root: 'tools', path: [0], span: null });
    expect(result).toEqual({ status: 'structural', valueKind: 'object' });
    expect(resolveSemanticLocator(PAYLOAD, { root: 'parameters', path: ['temperature'], span: null }))
      .toEqual({ status: 'structural', valueKind: 'number' });
  });

  it('null payload → root_missing (opaque/unavailable 前置)', () => {
    expect(resolveSemanticLocator(null, { root: 'systemPrompt', path: undefined, span: null }))
      .toEqual({ status: 'unavailable', reason: 'root_missing' });
  });

  it('missing root / missing path are reported, not guessed', () => {
    expect(resolveSemanticLocator(PAYLOAD, { root: 'input', path: undefined, span: null }))
      .toEqual({ status: 'unavailable', reason: 'root_missing' });
    expect(resolveSemanticLocator(PAYLOAD, { root: 'messages', path: [7, 'content'], span: null }))
      .toEqual({ status: 'unavailable', reason: 'path_missing' });
    expect(resolveSemanticLocator(PAYLOAD, { root: 'messages', path: ['not-an-index'], span: null }))
      .toEqual({ status: 'unavailable', reason: 'path_missing' });
  });

  it('span on a non-string target → not_text', () => {
    expect(resolveSemanticLocator(PAYLOAD, { root: 'parameters', path: ['temperature'], span: { start: 0, end: 1 } }))
      .toEqual({ status: 'unavailable', reason: 'not_text' });
  });

  it('span out of range / malformed → span_out_of_range', () => {
    expect(resolveSemanticLocator(PAYLOAD, { root: 'systemPrompt', path: undefined, span: { start: 0, end: 10_000 } }))
      .toEqual({ status: 'unavailable', reason: 'span_out_of_range' });
    expect(resolveSemanticLocator(PAYLOAD, { root: 'systemPrompt', path: undefined, span: { start: 5, end: 2 } }))
      .toEqual({ status: 'unavailable', reason: 'span_out_of_range' });
    expect(resolveSemanticLocator(PAYLOAD, { root: 'systemPrompt', path: undefined, span: { start: -1, end: 3 } }))
      .toEqual({ status: 'unavailable', reason: 'span_out_of_range' });
  });
});

describe('resolveProviderLocator (transport.body, same discipline)', () => {
  const PROVIDER_REQUEST = {
    transport: {
      protocol: 'openai',
      body: { system: 'You are Hana.', messages: [{ role: 'user', content: 'Q' }] },
    },
  };

  it('resolves a span through transport.body', () => {
    const result = resolveProviderLocator(PROVIDER_REQUEST, {
      path: ['messages', 0, 'content'], span: { start: 0, end: 1 },
    });
    expect(result).toEqual({ status: 'resolved', text: 'Q' });
  });

  it('structural without span; unavailable when transport/body missing', () => {
    expect(resolveProviderLocator(PROVIDER_REQUEST, { path: ['system'], span: null }))
      .toEqual({ status: 'structural', valueKind: 'string' });
    expect(resolveProviderLocator({}, { path: ['system'], span: null }))
      .toEqual({ status: 'unavailable', reason: 'root_missing' });
    expect(resolveProviderLocator(null, { path: [], span: null }))
      .toEqual({ status: 'unavailable', reason: 'root_missing' });
  });
});

describe('asSemanticInputProvenance guard', () => {
  it('accepts a well-shaped provenance and rejects garbage without throwing', () => {
    const provenance = { inputShape: 'chat_context', sections: [] };
    expect(asSemanticInputProvenance(provenance)).toBe(provenance);
    expect(asSemanticInputProvenance(null)).toBeNull();
    expect(asSemanticInputProvenance('nope')).toBeNull();
    expect(asSemanticInputProvenance({ sections: [] })).toBeNull();
    expect(asSemanticInputProvenance({ inputShape: 'chat_context', sections: 'x' })).toBeNull();
  });
});
