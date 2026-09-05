// @vitest-environment jsdom

import { describe, expect, it } from 'vitest';
import { parseUnifiedPatch } from '../../utils/unified-diff';

const SAMPLE_PATCH = [
  'diff --git a/a.md b/a.md',
  'index 1111111..2222222 100644',
  '--- a/a.md',
  '+++ b/a.md',
  '@@ -1,2 +1,3 @@',
  ' line1',
  '-line2',
  '+line2x',
  '+line3',
  '\\ No newline at end of file',
].join('\n');

describe('parseUnifiedPatch', () => {
  it('skips file headers before the first hunk and classifies body lines', () => {
    const rows = parseUnifiedPatch(SAMPLE_PATCH);
    expect(rows).toEqual([
      { kind: 'hunk', text: '@@ -1,2 +1,3 @@' },
      { kind: 'ctx', text: 'line1' },
      { kind: 'del', text: 'line2' },
      { kind: 'add', text: 'line2x' },
      { kind: 'add', text: 'line3' },
    ]);
  });

  it('does not mistake body lines starting with --- for headers after the first hunk', () => {
    const rows = parseUnifiedPatch('@@ -1,1 +1,2 @@\n ctx\n--- looks like header\n');
    expect(rows).toEqual([
      { kind: 'hunk', text: '@@ -1,1 +1,2 @@' },
      { kind: 'ctx', text: 'ctx' },
      { kind: 'del', text: '-- looks like header' },
    ]);
  });

  it('handles synthesized new-file patches', () => {
    const patch = [
      'diff --git a/new.txt b/new.txt',
      'new file mode 100644',
      '--- /dev/null',
      '+++ b/new.txt',
      '@@ -0,0 +1,2 @@',
      '+hello',
      '+world',
    ].join('\n');
    const rows = parseUnifiedPatch(patch);
    expect(rows.map(r => r.kind)).toEqual(['hunk', 'add', 'add']);
    expect(rows[1].text).toBe('hello');
  });

  it('returns empty for empty patch', () => {
    expect(parseUnifiedPatch('')).toEqual([]);
  });
});
