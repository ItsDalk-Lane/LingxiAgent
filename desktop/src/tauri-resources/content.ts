import MarkdownIt from 'markdown-it';
import * as mammoth from 'mammoth/mammoth.browser';
import ExcelJS from 'exceljs';
import { mergeMarkdownCoverIntoDocument, removeMarkdownCover } from '../react/utils/markdown-cover';
import type { ResourceBridge, Snapshot, SaveResult } from './bridge';

export type Preview = { kind: 'text' | 'markdown' | 'office' | 'info'; path: string; size: number; content: string; html?: string; snapshot?: Snapshot };
const markdown = new MarkdownIt({ html: false, linkify: false });
export const markdownHtml = (text: string) => markdown.render(text);
const escape = (value: string) => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function binaryBuffer(base64: string): ArrayBuffer {
  const data = atob(base64);
  const bytes = new Uint8Array(data.length);
  for (let i = 0; i < data.length; i += 1) bytes[i] = data.charCodeAt(i);
  return bytes.buffer;
}

export async function readPreview(path: string, bridge: ResourceBridge): Promise<Preview> {
  const info = await bridge.fileInfo(path);
  if (!info?.isFile) throw new Error('文件不存在或无法读取');
  const ext = path.split('.').pop()?.toLowerCase();
  if ((ext === 'docx' || ext === 'xlsx') && info.size <= 20 * 1024 * 1024) {
    const data = await bridge.readFileBase64(path);
    if (data == null) throw new Error('Office 文件读取失败');
    if (ext === 'docx') {
      const result = await mammoth.convertToHtml({ arrayBuffer: binaryBuffer(data) });
      return { kind: 'office', path, size: info.size, content: '', html: result.value };
    }
    const workbook = new ExcelJS.Workbook();
    await workbook.xlsx.load(binaryBuffer(data) as Parameters<typeof workbook.xlsx.load>[0]);
    const sheet = workbook.worksheets[0];
    if (!sheet) throw new Error('工作簿没有可预览的工作表');
    const rows: string[] = [];
    sheet.eachRow(row => {
      const cells: string[] = [];
      for (let i = 1; i <= sheet.columnCount; i += 1) cells.push(`<td>${escape(row.getCell(i).text)}</td>`);
      rows.push(`<tr>${cells.join('')}</tr>`);
    });
    return { kind: 'office', path, size: info.size, content: '', html: `<table>${rows.join('')}</table>` };
  }
  const snapshot = info.size <= 5 * 1024 * 1024 ? await bridge.readFileSnapshot(path) : null;
  if (snapshot) return { kind: ['md', 'mdx', 'markdown'].includes(ext || '') ? 'markdown' : 'text', path, size: info.size, content: snapshot.content, snapshot };
  // 小文件重新验证字节仍可读，避免把权限/读取失败误显示成“二进制信息卡”。
  if (info.size <= 20 * 1024 * 1024 && await bridge.readFileBase64(path) == null) throw new Error('文件读取失败');
  return { kind: 'info', path, size: info.size, content: '' };
}

export async function saveCover(path: string, snapshot: Snapshot, image: string | null, bridge: ResourceBridge): Promise<{ result: SaveResult; content: string }> {
  const transform = (text: string) => {
    if (image === null) return removeMarkdownCover(text);
    const merged = mergeMarkdownCoverIntoDocument(text, `---\ncover:\n  image: ${JSON.stringify(image)}\n---\n`);
    if (merged == null) throw new Error('封面格式无效');
    return merged;
  };
  let content = transform(snapshot.content);
  let result = await bridge.writeFileIfUnchanged(path, content, snapshot.version);
  if (!result.ok && result.conflict) {
    const fresh = await bridge.readFileSnapshot(path);
    if (fresh) {
      content = transform(fresh.content);
      result = await bridge.writeFileIfUnchanged(path, content, fresh.version);
    }
  }
  return { result, content };
}

export function retainResourceEvents(path: string, bridge: ResourceBridge, changed: () => Promise<void> | void, failed: (e: unknown) => void, pollMs = 250) {
  let stopped = false;
  let id: string | null = null;
  let keys: string[] = [];
  let sequence = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const poll = async () => {
    try {
      const result = await bridge.resourceEvents(sequence);
      if (stopped) return;
      const relevant = (result.events || []).some(event => ['resourceKey', 'oldResourceKey', 'newResourceKey'].some(key => keys.includes(String(event[key] || ''))) && ['resource.changed', 'resource.deleted', 'resource.renamed'].includes(String(event.type)));
      if (result.stale || relevant) await changed();
      if (typeof result.latestSequence === 'number') sequence = result.latestSequence;
    } catch (e) { if (!stopped) failed(e); }
    if (!stopped) timer = setTimeout(() => { void poll(); }, pollMs);
  };
  const ready = bridge.resourceSubscribe(path).then(async subscription => {
    id = subscription.subscriptionId; keys = subscription.resourceKeys;
    if (stopped) { await bridge.resourceUnsubscribe(id); return; }
    await poll();
  });
  return {
    ready,
    release: () => {
      stopped = true; if (timer) clearTimeout(timer);
      if (id) void bridge.resourceUnsubscribe(id).catch(failed);
    },
  };
}
