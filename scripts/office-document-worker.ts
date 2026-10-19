// 单次办公文档解析只读取 stdin 字节，不接收宿主路径、凭据或模型回调。
import { readOfficeDocument } from '../plugins/office/lib/read-document.ts';

// 仅办公字节帧使用此预算；普通 HTTP 与模型工具参数预算不随之扩大。
const MAX_REQUEST = 72 * 1024 * 1024;
const MAX_DOCUMENT = 50 * 1024 * 1024;
const MAX_OUTPUT = 16 * 1024 * 1024;
const chunks: Buffer[] = [];
let received = 0;
let accepted = false;
let finished = false;
function validBase64(value: string): boolean {
  if (value.length % 4 !== 0) return false;
  const padding = value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0;
  const end = value.length - padding;
  // 大帧按固定片段校验，避免重复分组正则在合法大输入上耗尽正则栈。
  for (let offset = 0; offset < end; offset += 64 * 1024) {
    if (!/^[A-Za-z0-9+/]+$/.test(value.slice(offset, Math.min(end, offset + 64 * 1024)))) return false;
  }
  return true;
}
function finish(value: unknown): void {
  if (finished) return;
  finished = true;
  process.stdout.write(JSON.stringify(value) + '\n', () => process.exit(0));
}
process.stdin.on('data', (chunk: Buffer) => {
  if (accepted) return;
  received += chunk.length;
  if (received > MAX_REQUEST) process.exit(2);
  chunks.push(chunk);
  if (!chunk.includes(10)) return;
  accepted = true;
  process.stdin.pause();
  const raw = Buffer.concat(chunks, received);
  chunks.length = 0;
  const newline = raw.indexOf(10);
  void (async () => {
    let id = '';
    try {
      if (raw.subarray(newline + 1).length) throw new Error('office_worker_extra_frame');
      const request = JSON.parse(raw.subarray(0, newline).toString('utf8'));
      id = typeof request.id === 'string' ? request.id : '';
      if (request.kind !== 'request' || request.proto !== 'lingxi-worker-rpc/1' || request.op !== 'office.read-document' || !id || request.resources?.length !== 0) throw new Error('office_worker_invalid_request');
      const args = request.args;
      if (!args || typeof args !== 'object' || Object.keys(args).some(key => !['bytesBase64', 'filename', 'inputOptions'].includes(key)) || typeof args.bytesBase64 !== 'string' || typeof args.filename !== 'string' || /[/\\\x00-\x1f]/.test(args.filename)) throw new Error('office_worker_invalid_arguments');
      if (args.bytesBase64.length > MAX_DOCUMENT * 4 / 3 + 4 || !validBase64(args.bytesBase64)) throw new Error('office_worker_invalid_bytes');
      const bytes = Buffer.from(args.bytesBase64, 'base64');
      if (!bytes.length || bytes.length > MAX_DOCUMENT) throw new Error('office_worker_document_size');
      const input = args.inputOptions;
      if (!input || typeof input !== 'object' || Array.isArray(input) || Object.keys(input).some(key => !['outputFormat', 'maxChars', 'pageRange', 'maxPages', 'sheetLimit', 'rowLimit', 'columnLimit'].includes(key))) throw new Error('office_worker_invalid_options');
      const result = await readOfficeDocument({ ...input, bytes, filename: args.filename });
      const response = { kind: 'result', id, ok: true, content: [{ type: 'text', text: JSON.stringify(result) }], claimed_files: [] };
      const size = Buffer.byteLength(JSON.stringify(response));
      if (!Number.isSafeInteger(request.max_output_bytes) || size > Math.min(MAX_OUTPUT, request.max_output_bytes)) throw new Error('office_worker_output_limit');
      finish(response);
    } catch (error) {
      // 解析错误只返回受限说明；不输出环境、栈或宿主路径。
      const message = error instanceof Error ? error.message : 'office_worker_parse_failed';
      finish({ kind: 'result', id, ok: false, error: message.slice(0, 1024), content: [], claimed_files: [] });
    }
  })();
});
process.stdin.on('end', () => { if (!accepted) process.exit(2); });
