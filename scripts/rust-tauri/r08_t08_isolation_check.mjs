#!/usr/bin/env node
/**
 * R08-T08 A15｜试用仍隔离生产（隔离条件校验，违反即非零退出）。
 *
 * 收尾任务A④重写：本检查此前只产摘要、从不失败（defaultOff=false 也退出 0）。
 * 现在的硬校验（任一违反 → exit 1，保留全部现场）：
 *   V1 真实 ~/.lingxi 前后一致——前置快照由本轮门首命令 r08_inputs_check 写入
 *      {EVIDENCE}/R08_INPUTS/real-home-before.json（受控前后摘要，不伪造历史基线：
 *      无前置快照=不可证未触碰 → 违反，除非 --before 显式提供早前真实快照）。
 *   V2 Rust 预览默认必须关：desktop/main.cjs 不得含默认 rust runtime。
 *   V3 默认配置文件无 rust 预览残留。
 *   V4 本轮真实命令目录实查：枚举同级 SUITES 各套件的 result.json 里的 argv，
 *      home/work/scratch/out 路径必须位于合成根（/private/tmp 或 TMPDIR），不得
 *      指向真实 home 或仓库工作树；缺 argv 的回执=不可证=违反（不再用固定声明）。
 *   V5 本进程环境不得携带指向真实 home 的 LINGXI_HOME。
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : null; };
const out = flag('--out');
if (!out || !existsSync(resolve(out, '..'))) { console.error('usage: --out <dir> [--before <snapshot>]'); process.exit(2); }
import { mkdirSync, writeFileSync } from 'node:fs';
mkdirSync(out, { recursive: true });

const realHome = join(homedir(), '.lingxi');
const repoRoot = resolve(new URL('.', import.meta.url).pathname, '..', '..');
const syntheticRoots = ['/private/tmp', '/tmp', tmpdir()];
const violations = [];
const notes = [];

function realHomeSnapshot() {
  const snap = { path: realHome };
  if (!existsSync(realHome)) { snap.exists = false; return snap; }
  snap.exists = true;
  const entries = {};
  for (const name of readdirSync(realHome).sort()) {
    const abs = join(realHome, name);
    const st = statSync(abs);
    entries[name] = st.isDirectory() ? 'dir' : `file:${st.size}:${Math.floor(st.mtimeMs)}`;
  }
  snap.topLevel = entries;
  const stampPath = join(realHome, 'data-epoch.json');
  if (existsSync(stampPath)) {
    try { snap.stampSha256 = createHash('sha256').update(readFileSync(stampPath)).digest('hex'); } catch (error) { snap.stampError = String(error); }
  }
  return snap;
}

const after = realHomeSnapshot();

// V1 前后比对：前置快照 = 本轮 r08_inputs_check 的受控采集（或 --before 显式提供）。
const defaultBefore = resolve(out, '..', '..', 'R08_INPUTS', 'real-home-before.json');
const beforePath = flag('--before') ?? (existsSync(defaultBefore) ? defaultBefore : null);
let before = null;
if (beforePath) {
  try { before = JSON.parse(readFileSync(beforePath, 'utf8')); } catch (error) { violations.push(`V1 before snapshot unparsable (${beforePath}): ${error}`); }
} else {
  violations.push('V1 no controlled before snapshot available (expected R08_INPUTS/real-home-before.json from this run or --before) — untouchability cannot be proven');
}
if (before) {
  const digest = (snap) => JSON.stringify({ exists: snap.exists, topLevel: snap.topLevel ?? null, stampSha256: snap.stampSha256 ?? null });
  const stable = digest(before) === digest(after);
  if (!stable) {
    const beforeKeys = new Set(Object.keys(before.topLevel ?? {}));
    const afterKeys = new Set(Object.keys(after.topLevel ?? {}));
    const added = [...afterKeys].filter((k) => !beforeKeys.has(k));
    const removed = [...beforeKeys].filter((k) => !afterKeys.has(k));
    const changed = [...beforeKeys].filter((k) => beforeKeys.has(k) && afterKeys.has(k) && before.topLevel[k] !== after.topLevel[k]);
    violations.push(`V1 real ~/.lingxi changed during the run: added=${JSON.stringify(added)} removed=${JSON.stringify(removed)} changed=${JSON.stringify(changed)}`);
  } else {
    notes.push('V1 real ~/.lingxi unchanged across this run (controlled before/after digests equal)');
  }
}

// V2 预览开关显式性：默认必须为旧内核（rust-local-service.cjs 的
// rustDesktopEnabled：env 缺省='node'，仅显式 'rust' 才启用；main.cjs 必须消费它）。
const gatingSource = join(repoRoot, 'desktop/src/shared/rust-local-service.cjs');
let gatingText = '';
try { gatingText = readFileSync(gatingSource, 'utf8'); } catch (error) { violations.push(`V2 gating source unreadable: ${error}`); }
const envDefaultNode = /LINGXI_DESKTOP_SERVER_RUNTIME\s*\|\|\s*['"]node['"]/.test(gatingText);
const explicitRustOnly = /return\s+runtime\s*===\s*['"]rust['"]/.test(gatingText);
const mainCjs = readFileSync(join(repoRoot, 'desktop/main.cjs'), 'utf8');
const mainConsumesGate = /rustDesktopEnabled/.test(mainCjs);
const defaultOff = envDefaultNode && explicitRustOnly && mainConsumesGate
  && !/serverRuntime\s*=\s*['"]rust['"]/.test(mainCjs);
if (!defaultOff) violations.push('V2 rust preview is not explicitly env-gated with a node default (rust-local-service.cjs / desktop/main.cjs)');
notes.push(`V2 preview env-gated default-off=${defaultOff} (envDefaultNode=${envDefaultNode} explicitRustOnly=${explicitRustOnly} mainConsumesGate=${mainConsumesGate})`);

// V3 默认配置无 rust 预览残留。
const defaultConfigPath = join(repoRoot, 'lib', 'config.json');
let configResidue = null;
if (existsSync(defaultConfigPath)) {
  const text = readFileSync(defaultConfigPath, 'utf8');
  configResidue = /["']?serverRuntime["']?\s*:\s*["']rust["']/.test(text);
  if (configResidue) violations.push('V3 lib/config.json carries a rust preview serverRuntime residue');
}

// V4 真实命令目录实查：同级 SUITES 回执 argv 逐一核对合成根。
const suitesRoot = resolve(out, '..');
const suiteChecks = [];
if (existsSync(suitesRoot)) {
  for (const name of readdirSync(suitesRoot).sort()) {
    const receiptPath = join(suitesRoot, name, 'result.json');
    if (!existsSync(receiptPath)) continue;
    let receipt;
    try { receipt = JSON.parse(readFileSync(receiptPath, 'utf8')); } catch (error) { violations.push(`V4 ${name}: result.json unparsable: ${error}`); continue; }
    if (!Array.isArray(receipt.argv)) { violations.push(`V4 ${name}: receipt lacks argv — synthetic-home usage cannot be proven (fixed declarations are not accepted)`); continue; }
    const suspicious = [];
    for (const element of receipt.argv) {
      if (typeof element !== 'string') continue;
      if (!/^--?(home|work|scratch|out|work-dir|backup-dir|pointer)(=|$)/.test(element) && !element.startsWith('/')) continue;
      const value = element.includes('=') ? element.split('=').slice(1).join('=') : null;
      const candidate = value ?? (/^\//.test(element) ? element : null);
      if (!candidate) continue;
      const abs = resolve(candidate);
      const inSynthetic = syntheticRoots.some((root) => abs === root || abs.startsWith(root + '/'));
      const inRealHome = abs === realHome || abs.startsWith(realHome + '/');
      const inRepo = abs === repoRoot || abs.startsWith(repoRoot + '/');
      if ((inRealHome || inRepo) && !inSynthetic) suspicious.push({ element, abs });
    }
    suiteChecks.push({ suite: name, argvChecked: receipt.argv.length, suspicious });
    if (suspicious.length) violations.push(`V4 ${name}: argv references real home or repo worktree: ${JSON.stringify(suspicious)}`);
  }
} else {
  notes.push('V4 no sibling SUITES directory (standalone run) — argv sweep skipped');
}

// V5 本进程环境不指向真实 home。
if (process.env.LINGXI_HOME && resolve(process.env.LINGXI_HOME).startsWith(realHome)) {
  violations.push('V5 process env LINGXI_HOME points into the real ~/.lingxi');
}

const summary = {
  schema: 'lingxi.r08.a15-isolation.v2',
  checkedAt: new Date().toISOString(),
  realHome,
  beforeSnapshot: beforePath,
  after: { exists: after.exists, topLevel: after.topLevel ?? null, stampSha256: after.stampSha256 ?? null },
  defaultOff,
  defaultConfigResidue: configResidue,
  suiteArgvSweep: suiteChecks,
  notes,
  violations,
  verdict: violations.length === 0 ? 'ISOLATED' : 'ISOLATION_VIOLATED',
};
writeFileSync(join(out, 'a15-isolation.json'), `${JSON.stringify(summary, null, 2)}\n`);
console.log(`A15_ISOLATION verdict=${summary.verdict} violations=${violations.length} defaultOff=${defaultOff}`);
for (const violation of violations) console.log(`VIOLATION ${violation}`);
process.exit(violations.length === 0 ? 0 : 1);
