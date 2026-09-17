import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { QuotedSelection } from '../../stores/input-slice';

import { formatQuotedSelectionForPrompt } from '../quoted-selection';

function makeSel(overrides: Partial<QuotedSelection> = {}): QuotedSelection {
  return {
    text: 'old line',
    sourceTitle: 'demo.ts',
    sourceKind: 'preview',
    sourceFilePath: '/tmp/demo.ts',
    lineStart: 2,
    lineEnd: 2,
    selectionAnchorKind: 'codemirror',
    charCount: 8,
    updatedAt: Date.now(),
    ...overrides,
  } as QuotedSelection;
}

describe('formatQuotedSelectionForPrompt', () => {
  it('带文件路径与行号时生成结构化头', () => {
    const out = formatQuotedSelectionForPrompt(makeSel());
    expect(out).toContain('demo.ts');
    expect(out).toContain('第2-2行');
    expect(out).toContain('/tmp/demo.ts');
    expect(out).toContain('[引用原文]');
  });
});
