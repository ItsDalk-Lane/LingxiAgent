import { MASKED_SECRET } from './secret-custody.ts';

/** 工具记录的展示投影；原始凭证、二进制内容和内部运行对象不进入聊天详情。 */
export const TOOL_PRESENTATION_TEXT_LIMIT = 64 * 1024;

export interface ToolPresentationDeferred {
  id: string;
  kind: 'tool_input' | 'tool_output' | 'tool_patch' | 'tool_file_content' | 'skill_content';
  size: number;
  preview?: string;
  available: true;
}

export interface ToolReadPresentation {
  path: string;
  startLine: number;
  totalLines?: number;
  displayedLines?: number;
  language?: string;
  truncated?: boolean;
}

export interface ToolSearchPresentation {
  kind: 'grep' | 'find' | 'ls';
  basePath?: string;
  files?: Array<{ path: string; matches?: Array<{ line?: number; text: string; context?: boolean }> }>;
  matchCount?: number;
  fileCount?: number;
  truncated?: boolean;
}

export interface ToolFileChangePresentation {
  path: string;
  patch?: string;
  content?: string;
  beforeAvailable: boolean;
  changeType?: 'created' | 'modified';
  reason?: string;
  added?: number;
  removed?: number;
  patchDeferred?: ToolPresentationDeferred;
  contentDeferred?: ToolPresentationDeferred;
  truncated?: boolean;
}

export interface ToolPresentationDetails {
  input?: string;
  inputDeferred?: ToolPresentationDeferred;
  inputTruncated?: boolean;
  output?: string;
  outputDeferred?: ToolPresentationDeferred;
  outputTruncated?: boolean;
  read?: ToolReadPresentation;
  search?: ToolSearchPresentation;
  fileChange?: ToolFileChangePresentation;
}

type PresentationContext = { toolName?: unknown; args?: unknown };
type PresentationResult = { toolName?: unknown; content?: unknown; details?: unknown; isError?: unknown };

const SECRET_KEY = /^(?:api[-_]?key|authorization|cookie|credentials?|password|secret|token|(?:access|refresh|bot|webhook|robot)[-_]?token|(?:app|client|corp|webhook|oauthClient|suite)[-_]?secret)$/i;
const SETTINGS_SECRET_KEY = /token|secret|password|api[_-]?key|authorization|credential/i;
const OMITTED_KEYS = new Set(['base64', 'base64Data', 'inlineData', 'thumbnail', 'signal', 'runtime', 'credentials']);
const SYNTHETIC_TOOLS = new Set([
  'knowledge_think', 'knowledge_read_part', 'knowledge_supplement', 'knowledge_answer',
  'knowledge_local_search', 'knowledge_research_plan', 'knowledge_research_round',
  'knowledge_research_worker', 'knowledge_research_progress', 'knowledge_research_review',
  'knowledge_research_synthesis',
]);

export function isSyntheticToolPresentation(toolName: unknown): boolean {
  return typeof toolName === 'string' && SYNTHETIC_TOOLS.has(toolName);
}

function recordOf(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown> : null;
}

/** 只按字段身份遮盖，不替换文件正文、补丁中的代码或普通字符串。 */
export function safeToolArguments(value: unknown): unknown {
  const seen = new WeakSet<object>();
  const visit = (entry: unknown, depth: number): unknown => {
    if (entry === null || typeof entry === 'string' || typeof entry === 'boolean') return entry;
    if (typeof entry === 'number') return Number.isFinite(entry) ? entry : null;
    if (typeof entry !== 'object') return undefined;
    if (seen.has(entry)) return '[Circular]';
    if (depth > 30) return '[Nested content omitted]';
    seen.add(entry);
    if (Array.isArray(entry)) {
      const output = entry.map(item => visit(item, depth + 1));
      seen.delete(entry);
      return output;
    }
    const descriptors = Object.getOwnPropertyDescriptors(entry);
    const settingsKey = descriptors.key && 'value' in descriptors.key ? descriptors.key.value : undefined;
    const sensitiveSetting = typeof settingsKey === 'string' && SETTINGS_SECRET_KEY.test(settingsKey);
    const output: Record<string, unknown> = Object.create(null);
    for (const [key, descriptor] of Object.entries(descriptors)) {
      if (!('value' in descriptor) || key === '__proto__' || key === 'constructor' || key === 'prototype') continue;
      if (SECRET_KEY.test(key) || (sensitiveSetting && ['value', 'before', 'after'].includes(key))) {
        output[key] = MASKED_SECRET;
      } else if (OMITTED_KEYS.has(key)) {
        output[key] = '[Content omitted]';
      } else {
        const projected = visit(descriptor.value, depth + 1);
        if (projected !== undefined) output[key] = projected;
      }
    }
    seen.delete(entry);
    return output;
  };
  return visit(value, 0);
}

export function safeToolInput(
  toolName: unknown,
  args: unknown,
  maxLength = TOOL_PRESENTATION_TEXT_LIMIT,
): Pick<ToolPresentationDetails, 'input' | 'inputTruncated'> | undefined {
  if (isSyntheticToolPresentation(toolName) || !recordOf(args)) return undefined;
  const input = JSON.stringify(safeToolArguments(args), null, 2);
  if (input.length <= maxLength) return { input };
  // 超长输入仍是合法 JSON；预览文本内的引号与反斜线最多使体积翻倍。
  const preview = input.slice(0, Math.max(0, Math.floor((maxLength - 80) / 2)));
  return { input: JSON.stringify({ truncated: true, preview }, null, 2), inputTruncated: true };
}

export function toolResultText(result: { content?: unknown } | null | undefined): string | null {
  if (!Array.isArray(result?.content)) return null;
  const text = result.content.flatMap((block: unknown) => {
    const item = recordOf(block);
    return item?.type === 'text' && typeof item.text === 'string' ? [item.text] : [];
  });
  return text.length ? text.join('\n') : null;
}

/** 校验每个统一补丁片段的行数后再统计，绝不把截断预览的计数当成总量。 */
export function toolPatchStats(patch: string): { added: number; removed: number } | undefined {
  let oldRemaining = 0;
  let newRemaining = 0;
  let hunks = 0;
  let added = 0;
  let removed = 0;
  for (const line of patch.split('\n')) {
    const header = /^@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@/.exec(line);
    if (header) {
      if (oldRemaining || newRemaining) return undefined;
      oldRemaining = header[1] === undefined ? 1 : Number(header[1]);
      newRemaining = header[2] === undefined ? 1 : Number(header[2]);
      hunks += 1;
      continue;
    }
    if (!oldRemaining && !newRemaining) {
      if ((line.startsWith('+') && !line.startsWith('+++ '))
        || (line.startsWith('-') && !line.startsWith('--- '))) return undefined;
      continue;
    }
    if (line.startsWith('\\')) continue;
    if (line.startsWith('+')) { newRemaining -= 1; added += 1; }
    else if (line.startsWith('-')) { oldRemaining -= 1; removed += 1; }
    else if (line.startsWith(' ')) { oldRemaining -= 1; newRemaining -= 1; }
    else return undefined;
    if (oldRemaining < 0 || newRemaining < 0) return undefined;
  }
  return hunks && !oldRemaining && !newRemaining ? { added, removed } : undefined;
}

function integer(value: unknown, minimum = 0): number | undefined {
  return typeof value === 'number' && Number.isInteger(value) && value >= minimum ? value : undefined;
}

function pathFromArgs(args: unknown): string {
  const record = recordOf(args);
  const path = record?.path ?? record?.file_path;
  return typeof path === 'string' ? path : '';
}

/** 只为旧记录解析已保存输出，不读取当前文件，也不把未知总量当成已知。 */
export function parseToolSearchOutput(kind: ToolSearchPresentation['kind'], output: string): ToolSearchPresentation {
  const files: NonNullable<ToolSearchPresentation['files']> = [];
  const byPath = new Map<string, NonNullable<ToolSearchPresentation['files']>[number]>();
  let matchCount = 0;
  let truncated = false;
  for (const text of output.split(/\r?\n/)) {
    if (!text.trim()) continue;
    if (/^\[.*(?:limit reached|truncated|more|omitted)/i.test(text)) { truncated = true; continue; }
    if (/^(?:No matches found|No files found matching pattern|\(empty directory\))$/.test(text.trim())) continue;
    if (kind !== 'grep') { files.push({ path: text }); continue; }
    const match = /^(.*?):(\d+): ?(.*)$/.exec(text) || /^(.*?)-(\d+)- ?(.*)$/.exec(text);
    if (!match) continue;
    const context = !/^(.*?):(\d+):/.test(text);
    let file = byPath.get(match[1]);
    if (!file) { file = { path: match[1], matches: [] }; byPath.set(match[1], file); files.push(file); }
    file.matches!.push({ line: Number(match[2]), text: match[3], ...(context ? { context: true } : {}) });
    if (!context) matchCount += 1;
  }
  return { kind, files, ...(kind === 'grep' ? { matchCount } : {}), fileCount: files.length, ...(truncated ? { truncated: true } : {}) };
}

function projectRead(raw: unknown, args: unknown, output: string): ToolReadPresentation | undefined {
  const record = recordOf(raw);
  const params = recordOf(args);
  const path = typeof record?.path === 'string' ? record.path : pathFromArgs(args);
  if (!path) return undefined;
  const startLine = integer(record?.startLine, 1) ?? integer(params?.offset, 1) ?? 1;
  let totalLines = integer(record?.totalLines);
  let displayedLines = integer(record?.displayedLines);
  let truncated = record?.truncated === true;
  // 旧 SDK 把空字节文件 split 成一行；只有确定从首行读到空内容时修正为空文件。
  if (output === '' && startLine === 1 && params?.limit !== 0 && (totalLines === undefined || totalLines <= 1)) {
    totalLines = 0;
    displayedLines = 0;
    truncated = false;
  }
  // 仅识别 SDK 在尾部追加且与本次范围一致的通知，避免为通知编造源文件行号。
  const shown = /\n\n\[Showing lines (\d+)-(\d+) of (\d+)(?: \([^\]\n]* limit\))?\. Use offset=(\d+) to continue\.\]$/.exec(output);
  const remaining = /\n\n\[(\d+) more lines in file\. Use offset=(\d+) to continue\.\]$/.exec(output);
  const notice = shown || remaining;
  if (notice) {
    const candidate = output.slice(0, notice.index);
    const lineCount = candidate.split('\n').length;
    const nextOffset = Number(shown ? shown[4] : remaining![2]);
    if (nextOffset === startLine + lineCount && (!shown || (Number(shown[1]) === startLine && Number(shown[2]) === nextOffset - 1))) {
      displayedLines ??= lineCount;
      totalLines ??= shown ? Number(shown[3]) : nextOffset - 1 + Number(remaining![1]);
      truncated = true;
    }
  }
  return {
    path, startLine,
    ...(totalLines !== undefined ? { totalLines } : {}),
    ...(displayedLines !== undefined ? { displayedLines } : {}),
    ...(typeof record?.language === 'string' ? { language: record.language.slice(0, 100) } : {}),
    ...(truncated ? { truncated: true } : {}),
  };
}

function projectSearch(raw: unknown, kind: ToolSearchPresentation['kind'], output: string | null, maxLength: number): ToolSearchPresentation {
  const record = recordOf(raw);
  const basePath = typeof record?.basePath === 'string' ? record.basePath : undefined;
  if (!record || !Array.isArray(record.files)) return {
    ...parseToolSearchOutput(kind, (output || '').slice(0, maxLength)),
    ...(basePath !== undefined ? { basePath } : {}),
  };
  const files: NonNullable<ToolSearchPresentation['files']> = [];
  let remaining = maxLength;
  let truncated = record.truncated === true;
  for (const value of record.files) {
    const file = recordOf(value);
    if (!file || typeof file.path !== 'string') continue;
    remaining -= file.path.length;
    if (remaining < 0) { truncated = true; break; }
    const matches: NonNullable<NonNullable<ToolSearchPresentation['files']>[number]['matches']> = [];
    if (Array.isArray(file.matches)) for (const item of file.matches) {
      const match = recordOf(item);
      if (!match || typeof match.text !== 'string') continue;
      if (remaining <= 0) { truncated = true; break; }
      const text = match.text.slice(0, remaining);
      if (text.length < match.text.length) truncated = true;
      remaining -= text.length;
      const line = integer(match.line, 1);
      matches.push({ text, ...(line !== undefined ? { line } : {}), ...(match.context === true ? { context: true } : {}) });
    }
    files.push({ path: file.path, ...(Array.isArray(file.matches) ? { matches } : {}) });
  }
  const matchCount = integer(record.matchCount);
  const fileCount = integer(record.fileCount);
  return { kind, ...(basePath !== undefined ? { basePath } : {}), files, ...(matchCount !== undefined ? { matchCount } : {}), ...(fileCount !== undefined ? { fileCount } : {}), ...(truncated ? { truncated: true } : {}) };
}

export function projectToolPresentationDetails(
  result: PresentationResult,
  context: PresentationContext = {},
  maxLength = TOOL_PRESENTATION_TEXT_LIMIT,
): ToolPresentationDetails | undefined {
  const toolName = typeof context.toolName === 'string' ? context.toolName : result.toolName;
  if (typeof toolName !== 'string' || !toolName || isSyntheticToolPresentation(toolName)) return undefined;
  const raw = recordOf(result.details);
  const input = safeToolInput(toolName, context.args, maxLength);
  let text = toolResultText(result);
  // 通用 JSON 返回按字段遮盖；文件工具正文保持真实，避免破坏代码与改动。
  if (text && !['read', 'edit', 'write', 'grep', 'find', 'ls', 'exec_command', 'write_stdin', 'bash'].includes(toolName)) {
    try { text = JSON.stringify(safeToolArguments(JSON.parse(text)), null, 2); } catch { /* 普通文本保留原样。 */ }
  }
  const details: ToolPresentationDetails = {
    ...input,
    ...(text !== null ? { output: text.slice(0, maxLength), ...(text.length > maxLength ? { outputTruncated: true } : {}) } : {}),
  };
  const imageRead = Array.isArray(result.content) && result.content.some((block: unknown) => recordOf(block)?.type === 'image');
  if (result.isError !== true && toolName === 'read' && text !== null && !imageRead
    && !/^(?:Read image file \[|\[Line \d+ is .*exceeds .*limit\.)/.test(text)) {
    const read = projectRead(raw?.read, context.args, text);
    if (read) details.read = read;
  }
  if (result.isError !== true && ['grep', 'find', 'ls'].includes(toolName)) {
    details.search = projectSearch(raw?.search, toolName as ToolSearchPresentation['kind'], text, maxLength);
  }
  if (result.isError !== true && ['edit', 'write'].includes(toolName)) {
    const change = recordOf(raw?.fileChange);
    const path = typeof change?.path === 'string' ? change.path : pathFromArgs(context.args);
    // 生产端明确省略过大的补丁时，不再从旧字段绕回被省略的全文。
    const patch = change ? change.patch : raw?.patch ?? raw?.diff;
    const content = change ? change.content : toolName === 'write' ? recordOf(context.args)?.content : undefined;
    const fullPatch = typeof raw?.patch === 'string' ? raw.patch : patch;
    const stats = typeof fullPatch === 'string' ? toolPatchStats(fullPatch) : undefined;
    if (path && (change || typeof patch === 'string' || typeof content === 'string')) {
      details.fileChange = {
        path,
        beforeAvailable: change ? change.beforeAvailable === true : typeof patch === 'string',
        ...stats,
        ...(typeof patch === 'string' ? { patch: patch.slice(0, maxLength) } : {}),
        ...(typeof content === 'string' ? { content: content.slice(0, maxLength) } : {}),
        ...(change?.changeType === 'created' || change?.changeType === 'modified' ? { changeType: change.changeType } : {}),
        ...(typeof change?.reason === 'string' ? { reason: change.reason.slice(0, 240) } : !change && typeof patch !== 'string' ? { reason: 'before_content_unavailable' } : {}),
        ...(change?.truncated === true || (typeof patch === 'string' && patch.length > maxLength) || (typeof content === 'string' && content.length > maxLength) ? { truncated: true } : {}),
      };
    }
  }
  return Object.keys(details).length ? details : undefined;
}

export function projectToolStartDetails(toolName: unknown, args: unknown): ToolPresentationDetails | undefined {
  const input = safeToolInput(toolName, args);
  if (!input) return undefined;
  const content = recordOf(args)?.content;
  const path = pathFromArgs(args);
  return {
    ...input,
    ...(toolName === 'write' && path && typeof content === 'string' ? {
      fileChange: {
        path, content: content.slice(0, TOOL_PRESENTATION_TEXT_LIMIT), beforeAvailable: false, reason: 'pending',
        ...(content.length > TOOL_PRESENTATION_TEXT_LIMIT ? { truncated: true } : {}),
      },
    } : {}),
  };
}
