import type { JSONContent } from '@tiptap/core';

export interface EditorFileRef {
  fileId?: string;
  path: string;
  name: string;
  isDirectory?: boolean;
  mimeType?: string;
  /** 附件随附的 base64 内容（音频/视频直传用）；仅内存态，不落盘 */
  base64Data?: string;
}

export interface EditorSessionRef {
  sessionId: string;
  label: string;
}

export interface EditorAgentMention {
  agentId: string;
  label: string;
}

/**
 * Walk TipTap JSON document, extract input badges and plain text.
 */
export function serializeEditor(json: JSONContent): {
  text: string;
  skills: string[];
  fileRefs: EditorFileRef[];
  sessionRefs: EditorSessionRef[];
  agentMentions: EditorAgentMention[];
} {
  const skills: string[] = [];
  const fileRefs: EditorFileRef[] = [];
  const sessionRefs: EditorSessionRef[] = [];
  const agentMentions: EditorAgentMention[] = [];

  function fileBadgeLabel(attrs: Record<string, unknown>): string {
    const name = typeof attrs.name === 'string' ? attrs.name : '';
    const filePath = typeof attrs.path === 'string' ? attrs.path : '';
    return name || filePath.split(/[\\/]/).pop() || filePath;
  }

  function paragraphHasText(content: JSONContent[] | undefined): boolean {
    return (content || []).some(child => child.type === 'text' && typeof child.text === 'string' && child.text.trim().length > 0);
  }

  function serializeInline(node: JSONContent, options: { emitFileBadgeText?: boolean } = {}): string {
    if (node.type === 'skillBadge' && node.attrs?.name) {
      skills.push(node.attrs.name as string);
      return '';
    }
    if (node.type === 'fileBadge' && node.attrs) {
      const path = typeof node.attrs.path === 'string' ? node.attrs.path : '';
      const label = fileBadgeLabel(node.attrs);
      if (label || path) {
        fileRefs.push({
          ...(typeof node.attrs.fileId === 'string' && node.attrs.fileId ? { fileId: node.attrs.fileId } : {}),
          path,
          name: label,
          isDirectory: node.attrs.isDirectory === true,
          ...(typeof node.attrs.mimeType === 'string' && node.attrs.mimeType ? { mimeType: node.attrs.mimeType } : {}),
        });
        if (options.emitFileBadgeText) {
          return `@${label}`;
        }
      }
      return '';
    }
    if (node.type === 'sessionBadge' && node.attrs) {
      const sessionId = typeof node.attrs.sessionId === 'string' ? node.attrs.sessionId.trim() : '';
      const label = typeof node.attrs.label === 'string' && node.attrs.label.trim()
        ? node.attrs.label.trim()
        : sessionId;
      if (sessionId) {
        sessionRefs.push({ sessionId, label });
        return `@${label}`;
      }
      return '';
    }
    if (node.type === 'agentBadge' && node.attrs) {
      const agentId = typeof node.attrs.agentId === 'string' ? node.attrs.agentId.trim() : '';
      const label = typeof node.attrs.label === 'string' && node.attrs.label.trim()
        ? node.attrs.label.trim()
        : agentId;
      if (agentId) {
        agentMentions.push({ agentId, label });
        return `@${label}`;
      }
      return '';
    }
    if (node.type === 'text' && node.text) {
      return node.text;
    }
    if (node.type === 'hardBreak') {
      return '\n';
    }
    return (node.content || []).map(child => serializeInline(child, options)).join('');
  }

  function serializeParagraph(node: JSONContent): string {
    const emitFileBadgeText = paragraphHasText(node.content);
    return (node.content || []).map(child => serializeInline(child, { emitFileBadgeText })).join('');
  }

  function listStart(node: JSONContent): number {
    const start = node.attrs?.start;
    return typeof start === 'number' && Number.isFinite(start) ? start : 1;
  }

  function serializeListItem(node: JSONContent, marker: string, indent: string): string[] {
    const markerContinuation = `${indent}${' '.repeat(marker.length)}`;
    const lines: string[] = [];
    let markerUsed = false;

    for (const child of node.content || []) {
      if (child.type === 'paragraph') {
        const paragraph = serializeParagraph(child);
        if (!markerUsed) {
          lines.push(`${indent}${marker}${paragraph}`);
          markerUsed = true;
        } else if (paragraph) {
          lines.push(`${markerContinuation}${paragraph}`);
        }
        continue;
      }

      if (child.type === 'bulletList' || child.type === 'orderedList') {
        if (!markerUsed) {
          lines.push(`${indent}${marker.trimEnd()}`);
          markerUsed = true;
        }
        lines.push(...serializeBlock(child, `${indent}  `));
        continue;
      }

      const childLines = serializeBlock(child, indent);
      if (childLines.length === 0) continue;
      if (!markerUsed) {
        lines.push(`${indent}${marker}${childLines[0].trimStart()}`);
        markerUsed = true;
        lines.push(...childLines.slice(1));
      } else {
        lines.push(...childLines);
      }
    }

    if (!markerUsed) {
      lines.push(`${indent}${marker.trimEnd()}`);
    }
    return lines;
  }

  function serializeList(node: JSONContent, indent: string): string[] {
    const ordered = node.type === 'orderedList';
    const start = ordered ? listStart(node) : 1;
    return (node.content || []).flatMap((child, index) => {
      if (child.type !== 'listItem') return serializeBlock(child, indent);
      const marker = ordered ? `${start + index}. ` : '- ';
      return serializeListItem(child, marker, indent);
    });
  }

  function serializeBlock(node: JSONContent, indent = ''): string[] {
    if (node.type === 'paragraph') {
      const paragraph = serializeParagraph(node);
      return paragraph ? [`${indent}${paragraph}`] : [];
    }
    if (node.type === 'bulletList' || node.type === 'orderedList') {
      return serializeList(node, indent);
    }
    if (node.type === 'doc') {
      return (node.content || []).flatMap(child => serializeBlock(child, indent));
    }
    if (node.content?.length) {
      return node.content.flatMap(child => serializeBlock(child, indent));
    }
    const inline = serializeInline(node);
    return inline ? [`${indent}${inline}`] : [];
  }

  const lines = serializeBlock(json);

  const text = lines.join('\n').replace(/\n+$/, '').trim();

  return { text, skills, fileRefs, sessionRefs, agentMentions };
}

/**
 * 粘贴保真：把剪贴板纯文本原样转成编辑器内容。
 *
 * 整段粘进一个段落，行与行之间用 hardBreak 分隔；空行即连续两个 hardBreak。
 * 序列化时 hardBreak 还原为 '\n'，文本逐字符还原（空行/缩进/围栏零丢失），
 * Markdown 的块结构（代码围栏/标题/列表/分隔线）在「编辑器 → 纯文本」往返中
 * 不再被富文本解析拍平。CRLF 统一归一为 LF。
 */
export function buildFaithfulPasteContent(text: string): JSONContent {
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  const content: JSONContent[] = [];
  lines.forEach((line, index) => {
    if (index > 0) content.push({ type: 'hardBreak' });
    if (line) content.push({ type: 'text', text: line });
  });
  if (content.length === 0) return { type: 'paragraph' };
  return { type: 'paragraph', content };
}
