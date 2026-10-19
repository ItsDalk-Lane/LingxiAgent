import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// 真实启动生产单操作 worker 与成熟解析库，不替换 stdin、解析器或退出过程。
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const worker = path.join(root, 'scripts/office-document-worker.ts');
const fixture = (name: string) => fs.readFileSync(path.join(root, 'tests/fixtures/document-extract', name));
const MiB = 1024 * 1024;

function request(bytes: Buffer, filename = 'notes.txt', inputOptions: Record<string, unknown> = {}) {
  return {
    kind: 'request', proto: 'lingxi-worker-rpc/1', id: 'actual-office-worker',
    op: 'office.read-document', deadline_unix_ms: Date.now() + 20_000,
    resources: [], max_output_bytes: 16 * MiB,
    args: { bytesBase64: bytes.toString('base64'), filename, inputOptions },
  };
}
function invokeFrame(frame: string | Buffer) {
  const result = spawnSync(process.execPath, ['--max-old-space-size=384', worker], {
    cwd: root, input: frame, encoding: 'utf8', timeout: 20_000, maxBuffer: 17 * MiB,
    env: { PATH: process.env.PATH, LANG: 'en_US.UTF-8' },
  });
  expect(result.signal, result.stderr).toBeNull();
  expect(result.error, result.stderr).toBeUndefined();
  return result;
}
function invoke(value: ReturnType<typeof request>) {
  const result = invokeFrame(JSON.stringify(value) + '\n');
  expect(result.status, result.stderr).toBe(0);
  const lines = result.stdout.trim().split('\n');
  expect(lines).toHaveLength(1);
  return JSON.parse(lines[0]);
}

describe('R07 Office document worker real subprocess contract', () => {
  it.each([
    ['sample.docx', 'html', 'docx', 'Quarterly Notes'],
    ['sample.xlsx', 'json', 'xlsx', 'North'],
    ['sample-text.pdf', 'markdown', 'pdf', 'Hello from PDF'],
  ])('parses actual %s bytes with no host path grant', (name, outputFormat, kind, text) => {
    const response = invoke(request(fixture(name), name, { outputFormat }));
    expect(response).toMatchObject({ kind: 'result', id: 'actual-office-worker', ok: true, claimed_files: [] });
    const parsed = JSON.parse(response.content[0].text);
    expect(parsed.kind).toBe(kind);
    expect(parsed.filePath).toBe(name);
    expect(JSON.stringify(parsed)).toContain(text);
  });

  it('reports scanned PDF as unsupported OCR instead of claiming extracted content', () => {
    const response = invoke(request(fixture('sample-scanned.pdf'), 'scan.pdf'));
    expect(response).toMatchObject({ ok: false, content: [], claimed_files: [] });
    expect(response.error).toContain('OCR is not supported');
  });

  it('accepts the actual 50MiB boundary as one frame and returns bounded truncated text', () => {
    const bytes = Buffer.alloc(50 * MiB, 'x');
    bytes.write('REAL LARGE DOCUMENT\n');
    const value = request(bytes, 'large.txt', { maxChars: 128 });
    expect(value.args.bytesBase64.length).toBeGreaterThan(32 * MiB);
    const response = invoke(value);
    expect(response.ok, response.error).toBe(true);
    const parsed = JSON.parse(response.content[0].text);
    expect(parsed).toMatchObject({ kind: 'text', filename: 'large.txt', truncated: true });
    expect(parsed.content).toContain('REAL LARGE DOCUMENT');
    expect(Buffer.byteLength(JSON.stringify(response))).toBeLessThan(2048);
  }, 30_000);

  it('rejects 50MiB plus one decoded byte even when the frame fits the dedicated transport cap', () => {
    const value = request(Buffer.alloc(50 * MiB + 1, 'x'), 'too-large.txt');
    expect(Buffer.byteLength(JSON.stringify(value))).toBeLessThan(72 * MiB);
    const response = invoke(value);
    expect(response).toMatchObject({ ok: false, error: 'office_worker_document_size', content: [], claimed_files: [] });
  }, 30_000);

  it('terminates an oversized 72MiB frame before JSON parsing without emitting a result', () => {
    const result = invokeFrame(Buffer.alloc(72 * MiB + 1, 'x'));
    expect(result.status, result.stderr).toBe(2);
    expect(result.stdout).toBe('');
  }, 30_000);

  it('does not accept an injected host path or authority as an input option', () => {
    const value = request(Buffer.from('authorized bytes only'));
    value.args.inputOptions = { filePath: '/private/owner.txt', role: 'owner' };
    const response = invoke(value);
    expect(response).toMatchObject({ ok: false, error: 'office_worker_invalid_options', content: [] });
  });

  it('keeps the host output budget even for a valid document', () => {
    const value = request(Buffer.from('a valid document with a known title'));
    value.max_output_bytes = 64;
    const response = invoke(value);
    expect(response).toMatchObject({ ok: false, error: 'office_worker_output_limit', content: [], claimed_files: [] });
  });
});
