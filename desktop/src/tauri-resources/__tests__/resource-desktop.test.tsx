import { afterEach, expect, test, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import ExcelJS from 'exceljs';
import JSZip from 'jszip';
import { ResourceDesktop } from '../App';
import { createResourceBridge, type Invoke, type Snapshot } from '../bridge';
import { readPreview, retainResourceEvents, saveCover } from '../content';

afterEach(() => { cleanup(); vi.useRealTimers(); });
const snapshot = (text = '# 原文\n', hash = 'first'): Snapshot => ({ content: text, version: { size: text.length, mtimeMs: 1, sha256: hash } });
function adapter(handler: (command: string, args: Record<string, unknown>) => unknown) {
  const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
  const invoke: Invoke = async <T,>(command: string, args: Record<string, unknown> = {}) => {
    calls.push({ command, args }); return await handler(command, args) as T;
  };
  return { calls, bridge: createResourceBridge(invoke) };
}
function readable(command: string) {
  if (command === 'file_info') return { path: '/proof/file.md', size: 20, isFile: true };
  if (command === 'read_file_snapshot') return snapshot();
  if (command === 'resource_subscribe') return { subscriptionId: 's1', resourceKeys: ['local_fs:/proof/file.md'] };
  if (command === 'resource_events') return { events: [], latestSequence: 0 };
  if (command === 'resource_unsubscribe') return { ok: true };
  throw new Error(`测试没有配置该调用：${command}`);
}

test('picker_returns_paths_cancel_has_no_file_io', async () => {
  let first = true;
  const { bridge, calls } = adapter(command => {
    expect(command).toBe('select_files');
    const selected = first ? ['/proof/selected.md'] : []; first = false; return selected;
  });
  render(<ResourceDesktop bridge={bridge} />);
  fireEvent.click(screen.getByTestId('select-files'));
  await screen.findByText('selected.md');
  expect(calls.map(call => call.command)).toEqual(['select_files']);
  expect(screen.queryByTestId('file-editor')).toBeNull();
  fireEvent.click(screen.getByTestId('select-files'));
  await waitFor(() => expect(calls).toHaveLength(2));
  expect(screen.getByText('selected.md')).toBeTruthy();
  expect(calls.every(call => call.command === 'select_files')).toBe(true);
});

test('viewer_renders_real_markdown_office_and_file_information', async () => {
  const markdown = adapter(readable);
  const result = await readPreview('/proof/file.md', markdown.bridge);
  expect(result.kind).toBe('markdown'); expect(result.snapshot?.content).toBe('# 原文\n');
  const zip = new JSZip();
  zip.file('[Content_Types].xml', '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>');
  zip.file('_rels/.rels', '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>');
  zip.file('word/document.xml', '<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>真实 Word 字节</w:t></w:r></w:p></w:body></w:document>');
  const docx = await zip.generateAsync({ type: 'base64' });
  const workbook = new ExcelJS.Workbook(); workbook.addWorksheet('Sheet').addRow(['真实表格', '<script>']);
  const xlsx = Buffer.from(await workbook.xlsx.writeBuffer()).toString('base64');
  for (const [ext, data, expected] of [['docx', docx, '真实 Word 字节'], ['xlsx', xlsx, '真实表格']]) {
    const port = adapter(command => {
      if (command === 'file_info') return { isFile: true, size: 2000 };
      if (command === 'read_file_base64') return data;
      throw new Error(command);
    });
    const office = await readPreview(`/proof/file.${ext}`, port.bridge);
    expect(office.kind).toBe('office'); expect(office.html).toContain(expected);
    if (ext === 'xlsx') { expect(office.html).toContain('&lt;script&gt;'); expect(office.html).not.toContain('<script>'); }
  }
  for (const size of [20, 21 * 1024 * 1024]) {
    const port = adapter(command => {
      if (command === 'file_info') return { isFile: true, size };
      if (command === 'read_file_snapshot') return null;
      if (command === 'read_file_base64') return 'AAEC';
      throw new Error(command);
    });
    expect(await readPreview('/proof/file.bin', port.bridge)).toMatchObject({ kind: 'info', size });
  }
  const failure = adapter(command => command === 'file_info' ? { isFile: true, size: 10 } : null);
  await expect(readPreview('/proof/no-access.txt', failure.bridge)).rejects.toThrow('文件读取失败');
});

test('viewer_consumes_resource_events_and_is_independent_readonly', async () => {
  let live = 'old'; let changed = false;
  const port = adapter(command => {
    if (command === 'viewer_request_load') return { filePath: '/proof/file.md', title: 'file.md', readOnly: true };
    if (command === 'read_file_snapshot') return snapshot(live, live);
    if (command === 'resource_events') return { events: changed ? [{ type: 'resource.changed', resourceKey: 'local_fs:/proof/file.md' }] : [], latestSequence: changed ? 1 : 0 };
    return readable(command);
  });
  const separate = render(<ResourceDesktop bridge={port.bridge} viewer />);
  await waitFor(() => expect((screen.getByTestId('file-editor') as HTMLTextAreaElement).value).toBe('old'));
  expect((screen.getByTestId('file-editor') as HTMLTextAreaElement).readOnly).toBe(true);
  expect(screen.queryByTestId('version-save')).toBeNull();
  expect(screen.getByTestId('markdown-preview').getAttribute('sandbox')).toBe('');
  live = 'external'; changed = true;
  await waitFor(() => expect((screen.getByTestId('file-editor') as HTMLTextAreaElement).value).toBe('external'));
  expect(port.calls.some(call => call.command === 'read_file_snapshot')).toBe(true);
  expect(port.calls.every(call => !call.command.startsWith('write_'))).toBe(true);
  separate.unmount();
  await waitFor(() => expect(port.calls.some(call => call.command === 'resource_unsubscribe')).toBe(true));
  let refreshes = 0;
  const irrelevant = adapter(command => {
    if (command === 'resource_events') return { events: [{ type: 'resource.changed', resourceKey: 'local_fs:/other/file' }], latestSequence: 9 };
    return readable(command);
  });
  const watch = retainResourceEvents('/proof/file.md', irrelevant.bridge, () => { refreshes += 1; }, e => { throw e; });
  await watch.ready; watch.release(); expect(refreshes).toBe(0);
});

test('saves_show_conflicts_and_cover_retries_only_once', async () => {
  const fresh = snapshot('外部正文\n', 'fresh'); let writes = 0;
  const port = adapter((command, args) => {
    if (command === 'write_file_if_unchanged') {
      writes += 1;
      if (writes === 2) { expect(args.expectedVersion).toEqual(fresh.version); expect(args.content).toContain('外部正文'); }
      return { ok: false, conflict: true, version: fresh.version };
    }
    if (command === 'read_file_snapshot') return fresh;
    return readable(command);
  });
  const cover = await saveCover('/proof/file.md', snapshot(), 'https://example.invalid/cover.png', port.bridge);
  expect(cover.result.ok).toBe(false); expect(cover.result.conflict).toBe(true);
  expect(writes).toBe(2); expect(port.calls.filter(call => call.command === 'read_file_snapshot')).toHaveLength(1);
  render(<ResourceDesktop bridge={port.bridge} />);
  fireEvent.change(screen.getByLabelText('本地文件路径'), { target: { value: '/proof/file.md' } });
  fireEvent.click(screen.getByTestId('open-path'));
  await screen.findByTestId('file-editor');
  fireEvent.change(screen.getByTestId('file-editor'), { target: { value: '用户编辑' } });
  fireEvent.click(screen.getByTestId('version-save'));
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('保存失败'));
  expect(screen.queryByText('已保存')).toBeNull();
  expect((screen.getByTestId('file-editor') as HTMLTextAreaElement).value).toBe('用户编辑');
  const success = adapter(command => command === 'write_file_if_unchanged' ? { ok: true, conflict: false, version: fresh.version } : readable(command));
  expect((await saveCover('/proof/file.md', snapshot(), null, success.bridge)).result.ok).toBe(true);
  expect(success.calls.filter(call => call.command === 'write_file_if_unchanged')).toHaveLength(1);
});

test('failure_results_never_register_copies_attachments_or_trash_success', async () => {
  const port = adapter(command => {
    if (['copy_file', 'write_file_binary', 'trash_item', 'write_file'].includes(command)) return false;
    return readable(command);
  });
  render(<ResourceDesktop bridge={port.bridge} />);
  fireEvent.change(screen.getByLabelText('本地文件路径'), { target: { value: '/proof/file.md' } });
  fireEvent.click(screen.getByTestId('open-path')); await screen.findByTestId('file-editor');
  fireEvent.click(screen.getByText('复制文件 / 写入附件'));
  fireEvent.change(screen.getByLabelText('目标文件路径'), { target: { value: '/proof/bad.bin' } });
  fireEvent.click(screen.getByText('复制当前文件'));
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('复制失败'));
  expect(document.querySelector('[data-path="/proof/bad.bin"]')).toBeNull();
  fireEvent.click(screen.getByText('写入附件'));
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('附件写入失败'));
  fireEvent.click(screen.getByTestId('trash-item'));
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('废纸篓失败'));
  expect(document.querySelector('[data-path="/proof/file.md"]')).not.toBeNull();
  fireEvent.click(screen.getByText('直接覆盖写入'));
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('写入失败'));
  expect(screen.queryByText('已直接写入')).toBeNull();
});
