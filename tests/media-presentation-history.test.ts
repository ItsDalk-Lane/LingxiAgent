import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Hono } from 'hono';
import { afterEach, describe, expect, it } from 'vitest';
import { createSessionsRoute } from '../server/routes/sessions.ts';
import { extractBlocks, resolveMediaGenerationBlocks } from '../server/block-extractors.ts';
import { normalizeContentBlocks } from '../desktop/src/react/utils/content-semantics.ts';

const roots: string[] = [];
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); });

const oldFile = { fileId: 'sf_media', filePath: '/synthetic/media.png', label: 'media.png', ext: 'png', size: 10, version: { mtimeMs: 1, size: 10, sha256: 'old' } };
const newFile = { ...oldFile, size: 20, version: { mtimeMs: 2, size: 20, sha256: 'new' } };
const options = { idPrefix: 'turn', turnLifecycle: 'sealed' as const };

function makeHistory(messages: any[], currentFile = newFile) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-media-history-'));
  roots.push(root);
  const agentsDir = path.join(root, 'agents');
  const sessionPath = path.join(agentsDir, 'hana', 'sessions', 'sample.jsonl');
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  const entries: any[] = [{ type: 'session', version: 3, id: 'sdk-fixture', timestamp: new Date().toISOString(), cwd: root }];
  messages.forEach((message, index) => entries.push({
    id: `entry-${index}`, parentId: index ? `entry-${index - 1}` : null,
    timestamp: new Date().toISOString(),
    ...(message.role === 'custom' ? { type: 'custom', customType: message.customType, data: message.data } : { type: 'message', message }),
  }));
  fs.writeFileSync(sessionPath, entries.map(entry => JSON.stringify(entry)).join('\n') + '\n');
  const engine: any = {
    agentsDir, currentSessionPath: sessionPath, deferredResults: null,
    getSessionFile: () => ({ ...currentFile, filePath: '/synthetic/relocated.png', status: 'available' }),
  };
  const app = new Hono();
  app.route('/api', createSessionsRoute(engine));
  return async () => {
    const response = await app.request(`/api/sessions/messages?path=${encodeURIComponent(sessionPath)}`);
    expect(response.status).toBe(200);
    return response.json() as Promise<any>;
  };
}

describe('媒体版本与历史的业务一致性', () => {
  it('真实结果提取保留版本，不把不同版本合并为一次通知', () => {
    const files = resolveMediaGenerationBlocks([{ type: 'media_generation', taskId: 'task', kind: 'image', status: 'pending' }],
      new Map([['task', { status: 'success', result: { sessionFiles: [oldFile] } }]]));
    const manual = extractBlocks('stage_files', { files: [newFile] }, null);
    expect(files[0].version).toEqual(oldFile.version);
    expect(manual[0].version).toEqual(newFile.version);
    expect(normalizeContentBlocks([...files, ...manual], options)).toHaveLength(2);
  });

  it('不同版本、独立任务和普通重复展示拥有独立且稳定的展示编号', () => {
    const blocks: any[] = [
      { type: 'file', ...oldFile, replacesTaskId: 'task-1' },
      { type: 'file', ...newFile },
      { type: 'file', ...oldFile, replacesTaskId: 'task-2' },
      { type: 'file', fileId: 'manual', filePath: '/synthetic/note.txt' },
      { type: 'file', fileId: 'manual', filePath: '/synthetic/note.txt' },
    ];
    const normalized = normalizeContentBlocks(blocks, options);
    expect(new Set(normalized.map(block => block.id)).size).toBe(5);
    expect(normalizeContentBlocks(normalized, options)).toEqual(normalized);
  });

  it.each(['image', 'video', 'speech'])('重开 %s 历史保留当时版本，只更新当前定位与可用状态', async kind => {
    const read = makeHistory([
      { role: 'assistant', content: [{ type: 'text', text: '已提交' }] },
      { role: 'toolResult', toolCallId: 'generate', toolName: `media_generate-${kind}`, content: [], details: { mediaGeneration: { kind, tasks: [{ taskId: 'task' }] } } },
      { role: 'toolResult', toolCallId: 'stage', toolName: 'stage_files', content: [], details: { files: [newFile] } },
      { role: 'custom', customType: 'hana-deferred-result', data: { schemaVersion: 1, taskId: 'task', status: 'success', type: `${kind}-generation`, result: { sessionFiles: [oldFile] } } },
    ]);
    const response = await read();
    const files = normalizeContentBlocks(response.blocks, options).filter(block => block.type === 'file');
    expect(files).toHaveLength(2);
    expect(files.map(file => file.version?.sha256).sort()).toEqual(['new', 'old']);
    expect(files.every(file => file.filePath === '/synthetic/relocated.png' && file.status === 'available')).toBe(true);
    expect(response.blocks.some((block: any) => block.type === 'media_generation')).toBe(false);
  });
});
