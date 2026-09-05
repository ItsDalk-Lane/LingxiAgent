/**
 * unified-diff — unified patch 文本 → 可渲染行
 *
 * 输入是 `git diff`（或未跟踪文件合成）的 patch 正文；文件头
 * （diff --git / index / --- / +++ …）只出现在第一个 hunk 之前，
 * 据此与正文里恰好以 "-- "/"++ " 开头的内容行区分。
 */

export type UnifiedDiffLineKind = 'add' | 'del' | 'ctx' | 'hunk';

export interface UnifiedDiffLine {
  kind: UnifiedDiffLineKind;
  text: string;
}

const FILE_HEADER_PREFIXES = [
  'diff ',
  'index ',
  '--- ',
  '+++ ',
  'new file mode',
  'deleted file mode',
  'old mode',
  'new mode',
  'similarity index',
  'dissimilarity index',
  'rename from',
  'rename to',
  'copy from',
  'copy to',
  'Binary files',
];

export function parseUnifiedPatch(patch: string): UnifiedDiffLine[] {
  const rows: UnifiedDiffLine[] = [];
  let inHunks = false;
  for (const line of patch.split('\n')) {
    if (line === '') continue; // 尾部换行产生的空串；真正的空上下文行是单个空格
    if (!inHunks && FILE_HEADER_PREFIXES.some(prefix => line.startsWith(prefix))) continue;
    if (line.startsWith('@@')) {
      inHunks = true;
      rows.push({ kind: 'hunk', text: line });
      continue;
    }
    if (line.startsWith('+')) {
      rows.push({ kind: 'add', text: line.slice(1) });
      continue;
    }
    if (line.startsWith('-')) {
      rows.push({ kind: 'del', text: line.slice(1) });
      continue;
    }
    if (line.startsWith('\\')) continue; // "\ No newline at end of file"
    if (!inHunks) continue; // 头部与首个 hunk 之间的其他杂行
    rows.push({ kind: 'ctx', text: line.startsWith(' ') ? line.slice(1) : line });
  }
  return rows;
}
