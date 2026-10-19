// 仅用本机隔离服务和合成值检查已知秘密字段；不连接供应商、不保存整机凭证。
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';

const root = process.cwd();
const binary = process.env.LINGXI_R07_SERVICE_BIN;
assert(binary, '必须明确指定本轮服务二进制');
const homeRoot = await fs.mkdtemp('/private/tmp/lingxi-model-metadata-proof-');
const home = path.join(homeRoot, 'home');
const workspace = path.join(homeRoot, 'workspace');
await fs.mkdir(home); await fs.mkdir(workspace);
const secrets = { api_key: 'owned-metadata-api-key-fixture', access_token: 'owned-metadata-access-token-fixture', refresh_token: 'owned-metadata-refresh-token-fixture' };
const model = { id: 'fixture-model', inputs: ['text'], extension: { ...secrets, ordinary: { keep: true } } };
const configPath = path.join(homeRoot, 'config.json');
await fs.writeFile(configPath, JSON.stringify({ home, workspace, providers: { fixture: { protocol: 'openai-completions', endpoint: 'http://127.0.0.1:9/v1', auth: { kind: 'none' }, models: [model] } }, models: {} }));
const sha = data => crypto.createHash('sha256').update(data).digest('hex');
const binaryBefore = sha(await fs.readFile(binary));
let stdout = '', stderr = '', owner = '';
const environment = { ...process.env, LINGXI_PRODUCT_DIR: path.join(root, 'lib') };
delete environment.LINGXI_HOME;
const child = spawn(binary, ['--config', configPath, '--bind', '127.0.0.1:0', '--shutdown-timeout-ms', '2000'], { cwd: homeRoot, env: environment, stdio: ['ignore', 'pipe', 'pipe'] });
const exited = new Promise(resolve => child.once('exit', (code, signal) => resolve({ code, signal })));
child.stdout.on('data', data => stdout += data);
child.stderr.on('data', data => stderr += data);
let failed = false;
let shutdown;
try {
  const address = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('隔离服务未就绪')), 30000);
    const check = () => { const match = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/); if (match) { clearTimeout(timer); resolve(match[1]); } };
    child.stdout.on('data', check);
    child.once('exit', () => { clearTimeout(timer); reject(new Error('隔离服务提前退出：' + stderr)); });
    check();
  });
  owner = JSON.parse(await fs.readFile(path.join(home, 'lingxi-service/local-token.json'), 'utf8')).token;
  const base = `http://${address}`;
  const created = await fetch(base + '/lingxi/v1/agents', { method: 'POST', headers: { authorization: `Bearer ${owner}`, 'content-type': 'application/json' }, body: JSON.stringify({ id: 'a', name: '隔离模型设置' }) });
  assert.equal(created.status, 200, '实际创建隔离助手必须成功：' + await created.text());
  const writtenModel = { ...model, name: '实际模型保存确认' };
  const save = await fetch(base + '/lingxi/v1/agents/a/config', { method: 'PUT', headers: { authorization: `Bearer ${owner}`, 'content-type': 'application/json' }, body: JSON.stringify({ providers: { fixture: { models: [writtenModel] } } }) });
  const saved = await save.json();
  assert.equal(save.status, 200, '实际设置保存必须成功：' + JSON.stringify(saved));
  assert.equal(saved.modelConfiguration?.saved, true, '必须实际提交模型来源保存');
  assert.equal(saved.modelConfiguration?.applied, true, '必须实际发布模型变更');
  const observations = [];
  for (const route of ['/lingxi/v1/providers/summary', '/lingxi/v1/settings/config', '/lingxi/v1/settings/export']) {
    const response = await fetch(base + route, { headers: { authorization: `Bearer ${owner}` } });
    const text = await response.text();
    assert.equal(response.status, 200, route + '实际读面必须可用');
    const exposedKnownFields = Object.entries(secrets).filter(([, value]) => text.includes(value)).map(([key]) => key);
    observations.push({ route, status: response.status, exposedKnownFields, responseSha256: sha(text), ordinaryExtensionPreserved: text.includes('"keep":true') || /"keep"\s*:\s*true/.test(text) });
    if (exposedKnownFields.length) failed = true;
  }
  const after = JSON.parse(await fs.readFile(configPath, 'utf8'));
  assert.equal(after.providers.fixture.models[0].name, writtenModel.name, '真实模型变更必须进入唯一来源');
  assert.deepEqual(after.providers.fixture.models[0].extension, model.extension, '脱敏只能改变读面，不得丢失合法兼容字段或静默清空来源');
  console.log(JSON.stringify({ scope: 'OWNED_LOCAL_HTTP_METADATA_REDACTION_NOT_FINAL_R07', homeRoot, binarySha256Before: binaryBefore, actualSettingsWriteStatus: save.status, actualModelSourceSaved: saved.modelConfiguration.saved, actualModelSourceApplied: saved.modelConfiguration.applied, observations, sourceExtensionPreserved: true }));
} finally {
  child.kill('SIGTERM');
  let timer;
  shutdown = await Promise.race([exited, new Promise(resolve => { timer = setTimeout(() => resolve({ timeout: true }), 7000); })]);
  clearTimeout(timer);
  if (shutdown.timeout) { child.kill('SIGKILL'); await exited; failed = true; }
  const binaryAfter = sha(await fs.readFile(binary));
  console.log(JSON.stringify({ shutdown, binarySha256After: binaryAfter, binaryUnchanged: binaryBefore === binaryAfter, stderrSha256: sha(stderr.replaceAll(owner || '[no-owner]', '[owner-redacted]')) }));
  if (binaryBefore !== binaryAfter || shutdown.code !== 0) failed = true;
}
process.exitCode = failed ? 1 : 0;
