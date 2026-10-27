#!/usr/bin/env node
/**
 * R08-T08 A16｜Tauri 对照基线固定（交接版本清单）。
 * - React/Rust/数据 fixture 版本与摘要明确，不依赖漂移 main：
 *   git HEAD（本阶段全部工作未提交，工作区为唯一副本——HEAD 即基线锚）；
 *   Rust 二进制 sha256；desktop/dist-renderer 树摘要；contract-versions 摘要；
 *   夹具 v3 锚（80a8d2c2…feaa）与 v2 大规模锚（40f42dc1…）；阶段图 inputDigest。
 */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
const out = args[args.indexOf('--out') + 1];
if (!out) { console.error('usage: --out <dir>'); process.exit(2); }
mkdirSync(out, { recursive: true });
const sha256 = (p) => createHash('sha256').update(readFileSync(p)).digest('hex');
function treeDigest(root) {
  if (!existsSync(root)) return null;
  const h = createHash('sha256');
  const walk = (dir) => {
    for (const name of readdirSync(dir).sort()) {
      const abs = join(dir, name);
      if (statSync(abs).isDirectory()) walk(abs);
      else h.update(name).update(readFileSync(abs));
    }
  };
  walk(root);
  return h.digest('hex');
}
const head = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
const manifest = {
  schema: 'lingxi.r08.a16-versions.v1',
  generatedAt: new Date().toISOString(),
  gitHead: head,
  branch: execFileSync('git', ['branch', '--show-current'], { encoding: 'utf8' }).trim(),
  uncommittedWorkspaceIsSoleCopy: true,
  node: process.version,
  packageVersion: JSON.parse(readFileSync('package.json', 'utf8')).version,
  rustBinary: existsSync('rust/target/debug/lingxi-service') ? sha256('rust/target/debug/lingxi-service') : null,
  rendererDist: treeDigest('desktop/dist-renderer'),
  contractVersions: existsSync('shared/contract-versions.json') ? sha256('shared/contract-versions.json') : null,
  fixtureAnchors: {
    v3Base: '80a8d2c2640e2b2974b299a10f36679bc19daf0911e51f82f0c43399cb32feaa',
    v2Bulk: '40f42dc1fdb7dc026ee1ce6444914a02a085c12c53b7598e70fc453fdb388c9e',
  },
  stageMapInputDigest: (() => {
    try { return JSON.parse(readFileSync('rust/crates/xtask/src/stage_maps/R08.json', 'utf8')).inputDigestSha256; } catch { return null; }
  })(),
  handoffBoundaries: {
    noDriftingMain: 'R08 成果全部锚定上述 HEAD + 工作区副本；R09 起以本清单为对照基线',
    protocolFreeze: 'R09 不得趁机重改模型协议或数据架构（任务书 T08 步骤 4）',
  },
};
writeFileSync(join(out, 'a16-versions.json'), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`A16_VERSIONS head=${head.slice(0, 12)} rust=${manifest.rustBinary?.slice(0, 12)} renderer=${manifest.rendererDist?.slice(0, 12)}`);
