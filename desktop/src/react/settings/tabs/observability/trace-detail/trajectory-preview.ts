/**
 * trajectory-preview.ts — 有界的 Markdown→单行文本投影。
 *
 * 移植自 dsh-desktop packages/client/ui-trajectory/src/client/trajectory-preview.ts
 * （MIT）。适配点：extractMarkdownPlainText（dsh ui-primitives）换成轻量本地
 * markdown 语法剥离（仅服务单行预览，不做完整解析）。
 */

const PREVIEW_SOURCE_CHARACTERS = 2_048;
const PREVIEW_OUTPUT_CHARACTERS = 512;

function stripMarkdownSyntax(source: string): string {
  return source
    // 围栏代码块保留内部文字（单行预览里代码比围栏标记更有信息量）。
    .replace(/```[^\n]*\n?/g, ' ')
    .replace(/~~~[^\n]*\n?/g, ' ')
    // 图片只剩 alt 文本。
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    // 链接保留文字。
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    // 行内代码标记。
    .replace(/`([^`]*)`/g, '$1')
    // 强调/加粗/删除线标记。
    .replace(/(\*\*|__|\*|_|~~)([^*_~]+)\1/g, '$2')
    // 标题与引用前缀。
    .replace(/^\s{0,3}(#{1,6}|>)\s?/gm, '')
    // 列表标记。
    .replace(/^\s*[-*+]\s+/gm, '')
    .replace(/^\s*\d+[.)]\s+/gm, '')
    // 水平线。
    .replace(/^\s*([-*_]\s*){3,}$/gm, ' ')
    // 表格分隔线。
    .replace(/^\s*\|?[\s:|-]+\|[\s:|-]*$/gm, ' ')
    .replace(/\|/g, ' ');
}

/**
 * 构建有界的单行预览，不解析完整 Markdown 文档。
 */
export function trajectoryPreviewText(text: string): string {
  const source = text.slice(0, PREVIEW_SOURCE_CHARACTERS);
  const compact = stripMarkdownSyntax(source).replace(/\s+/g, ' ').trim();
  const preview = compact.slice(0, PREVIEW_OUTPUT_CHARACTERS).trimEnd();
  return source.length < text.length || preview.length < compact.length
    ? `${preview}…`
    : preview;
}
