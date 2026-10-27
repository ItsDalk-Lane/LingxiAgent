// 媒体选项只从现役纯供应商声明派生，不加载旧执行内核或用户配置。
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { buildSync } from 'esbuild';

const definitions = [
  ['openai', 'openaiPlugin'], ['openai-codex-oauth', 'openaiCodexOAuthPlugin'],
  ['volcengine', 'volcenginePlugin'], ['minimax', 'minimaxPlugin'],
  ['dashscope', 'dashscopePlugin'], ['gemini', 'geminiPlugin'], ['agnes', 'agnesPlugin'],
];
const rootDefault = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export async function generateMediaMetadata({ rootDir = rootDefault, check = false } = {}) {
  const imports = definitions.map(([file, name]) => `import {${name}} from './lib/providers/${file}.ts';`).join('\n');
  const contents = imports + `\nexport default [${definitions.map(([, name]) => name).join(',')}].map(p=>({providerId:p.id,displayName:p.displayName,media:Object.fromEntries(['imageGeneration','videoGeneration','speechGeneration','speechRecognition'].filter(k=>p.capabilities?.media?.[k]).map(k=>[k,p.capabilities.media[k]]))}));`;
  const result = buildSync({ stdin: { contents, resolveDir: rootDir, sourcefile: 'media-metadata-pure.ts' }, bundle: true, write: false, platform: 'node', format: 'esm', metafile: true, logLevel: 'silent' });
  const inputs = Object.keys(result.metafile.inputs);
  const allowed = new Set([...definitions.map(([file]) => `lib/providers/${file}.ts`), 'lib/providers/media-schema-helpers.ts', 'media-metadata-pure.ts']);
  if (inputs.some(input => !allowed.has(input))) throw new Error('Media metadata includes an undeclared input');
  const bundle = result.outputFiles[0].text;
  if (/(?:process\.|\bfetch\(|readFile|writeFile|node:fs|\bfrom\s+["'])/.test(bundle)) throw new Error('Media metadata contains an unexpected runtime effect');
  const data = (await import('data:text/javascript;base64,' + Buffer.from(bundle).toString('base64'))).default;
  const json = JSON.stringify(data);
  const generated = '//! 由现役纯供应商声明生成的媒体选项；执行、授权及凭据仍由 Rust 宿主管理。\n//! 来源限定 lib/providers/ 的七个现役媒体供应商及纯 schema helpers。\npub const DECLARATIONS_JSON: &str = r###"' + json + '"###;\n';
  const target = path.join(rootDir, 'rust/crates/lingxi-service/src/media_metadata.rs');
  if (check) {
    if (fs.readFileSync(target, 'utf8') !== generated) throw new Error('Media metadata differs from the current provider declarations; regenerate it');
  } else {
    fs.writeFileSync(target, generated);
  }
  return { target, inputs, providers: data.length, models: data.flatMap(provider => Object.values(provider.media).flatMap(capability => capability.models ?? [])).length, sha256: createHash('sha256').update(generated).digest('hex') };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = await generateMediaMetadata({ check: process.argv.includes('--check') });
  process.stdout.write(JSON.stringify(result) + '\n');
}
