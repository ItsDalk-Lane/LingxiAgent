import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, afterAll, describe, expect, it } from 'vitest';
import { buildMediaWorker } from '../scripts/build-media-worker.mjs';
import { generateMediaMetadata } from '../scripts/generate-media-metadata.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
let output: string;
let worker: string;
let snapshot: any;
function invoke(op: string, args: unknown) {
  const result = spawnSync(process.execPath, [worker], {
    cwd: root, input: JSON.stringify({ proto: 'lingxi-worker-rpc/1', kind: 'request', id: 'actual-media-sdk-worker', op, args, deadline_unix_ms: Date.now() + 20_000, resources: [], max_output_bytes: 1024 * 1024 }) + '\n',
    encoding: 'utf8', timeout: 20_000, maxBuffer: 2 * 1024 * 1024,
    env: { PATH: process.env.PATH, LANG: 'en_US.UTF-8' },
  });
  expect(result.status, result.stderr).toBe(0);
  expect(result.error).toBeUndefined();
  const frames = result.stdout.trim().split('\n');
  expect(frames).toHaveLength(1);
  return JSON.parse(frames[0]);
}
beforeAll(() => {
  output = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-r07-media-sdk-'));
  worker = buildMediaWorker({ rootDir: root, bundleOutDir: output });
  const fixtures = fs.readFileSync(path.join(root, 'tests/jimeng-cli-capabilities.test.ts'), 'utf8');
  const outputs = Object.fromEntries([
    ['text2image', 'TEXT_TO_IMAGE_HELP'], ['image2image', 'IMAGE_TO_IMAGE_HELP'], ['text2video', 'TEXT_TO_VIDEO_HELP'], ['image2video', 'IMAGE_TO_VIDEO_HELP'],
  ].map(([mode, name]) => [mode, fixtures.split(`const ${name} = \``)[1].split('`')[0]]));
  const actual = invoke('jimeng.capabilities', { outputs, versionOutput: JSON.stringify({ version: 'synthetic-1.0.0' }), fingerprint: { path: '/synthetic/dreamina', mtimeMs: 1, size: 1 } });
  expect(actual.ok).toBe(true);
  snapshot = actual.structured;
});
afterAll(() => { if (output) fs.rmSync(output, { recursive: true, force: true }); });

describe('真实单操作媒体SDK包', () => {
  it('保留现役静态声明的实际模型、模式和约束，生成物不能漂移', async () => {
    const result = await generateMediaMetadata({ rootDir: root, check: true });
    expect(result.providers).toBe(7);
    expect(result.models).toBeGreaterThan(0);
    expect(result.inputs.every((input: string) => input.startsWith('lib/providers/') || input === 'media-metadata-pure.ts')).toBe(true);
  });
  it('从四份真实CLI帮助响应推导模型，不造默认候选', () => {
    expect(snapshot.media.imageGeneration.defaultModelId).toBe('jimeng-image-5.0');
    expect(snapshot.media.videoGeneration.defaultModelId).toBe('seedance2.0fast');
    expect(snapshot.media.imageGeneration.models.find((model: any) => model.id === 'jimeng-image-5.0').modes.some((mode: any) => mode.id === 'image2image')).toBe(true);
  });
  it.each(['image', 'video'])('实际成熟SDK构造%s参数，提交命令必须不等待生成', kind => {
    const result = invoke('jimeng.submit-plan', { kind, snapshot, params: { prompt: '本地纯参数验证', image: ['/synthetic/reference.png'] } });
    expect(result.ok).toBe(true);
    expect(result.structured.argv[0]).toBe(kind === 'image' ? 'image2image' : 'image2video');
    const poll = result.structured.argv.indexOf('--poll');
    expect(result.structured.argv[poll + 1]).toBe('0');
  });
  it('只返回供应商实际submit_id，不把缺失编号补成成功', () => {
    expect(invoke('jimeng.parse-result', { output: '{"submit_id":"actual-synthetic-id","gen_status":"querying"}' }).structured.submitId).toBe('actual-synthetic-id');
    expect(invoke('jimeng.parse-result', { output: '{"gen_status":"querying"}' }).structured.submitId).toBe('');
  });
  it('拒绝未知操作、伪模型和意外凭据字段，不回显秘密', () => {
    for (const [op, args] of [
      ['not-a-media-operation', {}],
      ['jimeng.submit-plan', { kind: 'image', snapshot, params: { prompt: 'x', modelId: 'invented-model' } }],
      ['jimeng.submit-plan', { kind: 'image', snapshot, params: { prompt: 'x', token: 'synthetic-secret-must-not-appear' } }],
    ] as const) {
      const result = invoke(op, args);
      expect(result.ok).toBe(false);
      expect(JSON.stringify(result)).not.toContain('synthetic-secret-must-not-appear');
    }
  });
});
