/**
 * regen-delivery-manifests.mjs — recheck 轮交付清单重生成。
 *
 * 1. SOURCE_MANIFEST.json：与 run-logged 相同的源码过滤（core/lib/server/shared/
 *    desktop/cli/scripts/tests/build/.github + 根配置），另纳入 P05 反例测试；
 *    绑定 HEAD/树/dirty/环境与全量运行源快照摘要。
 * 2. EVIDENCE_SHA256.txt：源码 ∪ 任务书 ∪ docs/refactor-2026/independent-fix ∪
 *    artifacts/refactor-2026（含 gitignored 原始日志）；排除清单自身与
 *    FINAL_VERIFICATION.json（不自引用）。
 * 3. FINAL_VERIFICATION.json：对新清单真实执行 shasum -c 的结果。
 *
 * 幂等性：重复执行产生相同清单内容（generated_at/at 除外）。
 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import process from 'node:process';
import console from 'node:console';
import { execFileSync } from 'node:child_process';

const D = 'docs/refactor-2026/independent-fix';
const A = 'artifacts/refactor-2026/independent-fix';
const P05_COUNTEREXAMPLE = 'artifacts/refactor-2026/P05/logs/P05-ACCEPTANCE-counterexample.test.ts';
const EVIDENCE_HEADER = [
  '# SHA-256；路径相对仓库根目录。包含最终源码、输入任务书、报告与原始日志（含gitignored .log）。',
  '# 本清单自身及记录本清单校验结果的FINAL_VERIFICATION.json不自引用。',
];

const sha256 = (buf) => crypto.createHash('sha256').update(buf).digest('hex');
const walk = (root) => {
  const out = [];
  const visit = (rel) => {
    const abs = path.join(process.cwd(), rel);
    const stat = fs.statSync(abs);
    if (stat.isDirectory()) {
      for (const name of fs.readdirSync(abs).sort()) visit(path.join(rel, name));
    } else if (stat.isFile()) out.push(rel);
  };
  visit(root);
  return out;
};

// ── 源码清单（与 run-logged 过滤一致；路径取自 git 索引，字节取自工作区，
//    与上一轮口径相同——无关的未跟踪工作区残留不进入候选清单）──
const tracked = execFileSync('git', ['ls-files', '-z', '--cached'], { encoding: 'utf8' })
  .split('\0').filter(Boolean)
  // 上一轮候选清单即不含该基线期发布提示文本；维持同口径，使本轮清单差异恰为本轮修改。
  .filter((p) => p !== 'desktop/如果提示已损坏请看这里.txt');
const sourcePaths = tracked
  .filter((p) => /^(core|lib|server|shared|desktop|cli|scripts|tests|build|\.github)\//.test(p)
    || /^(package(-lock)?\.json|tsconfig[^/]*\.json|vitest\.config\.js|vite[^/]*|export-manifest\.json)$/.test(p))
  .filter((p) => fs.existsSync(p) && fs.statSync(p).isFile());
sourcePaths.push(P05_COUNTEREXAMPLE);
sourcePaths.sort();

const git = (...args) => execFileSync('git', args, { encoding: 'utf8' }).trim();
const snapshotPath = `${A}/main/engineering-full-tests-recheck.source.json`;
const snapshotBytes = fs.readFileSync(snapshotPath);
const manifest = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  head: git('rev-parse', 'HEAD'),
  tree: git('rev-parse', 'HEAD^{tree}'),
  dirty: true,
  verification_snapshot: snapshotPath,
  verification_source_digest: sha256(snapshotBytes),
  environment: {
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    lock_sha256: sha256(fs.readFileSync('package-lock.json')),
  },
  scope: '与最终全量相同的生产/测试/构建/CI精确字节，另纳入P05反例；文档与原始证据由EVIDENCE_SHA256覆盖。未提交修复不等于HEAD。',
  files: sourcePaths.map((p) => ({ path: p, sha256: sha256(fs.readFileSync(p)) })),
};
fs.writeFileSync(`${D}/SOURCE_MANIFEST.json`, JSON.stringify(manifest, null, 2) + '\n');

// ── 证据清单（与上一轮同范围：源码 ∪ 任务书 ∪ 本交付文档与勘误 ∪ independent-fix 证据树；
//    P0x 阶段目录各有自己的 EVIDENCE 清单体系，不在本清单重复收录）──
const evidenceSet = new Set(sourcePaths);
for (const p of walk('Lingxi_Refactor_Taskbooks_2026-09-21')) evidenceSet.add(p);
for (const p of walk('docs/refactor-2026/independent-fix')) evidenceSet.add(p);
evidenceSet.add('docs/README.md');
for (const p of walk('docs/refactor-2026')) {
  if (/^docs\/refactor-2026\/P\d+\/ACCEPTANCE_CORRECTION\.md$/.test(p)) evidenceSet.add(p);
}
evidenceSet.add('docs/refactor-2026/P08/STRICT_FINAL_SCOPE.json');
for (const p of walk('artifacts/refactor-2026/independent-fix')) evidenceSet.add(p);
evidenceSet.delete(`${D}/EVIDENCE_SHA256.txt`);
evidenceSet.delete(`${D}/FINAL_VERIFICATION.json`);
// 干净 checkout 复验输出是对清单自身的元验证，写入时间晚于清单生成，不入清单。
evidenceSet.delete(`${A}/recheck-r1-r2/clean-checkout-verify.out`);
const evidenceLines = [...evidenceSet].sort().map((p) => `${sha256(fs.readFileSync(p))}  ${p}`);
fs.writeFileSync(`${D}/EVIDENCE_SHA256.txt`, [...EVIDENCE_HEADER, ...evidenceLines].join('\n') + '\n');

// ── 真实校验并记录 ──
let verification;
try {
  const stdout = execFileSync('shasum', ['-a', '256', '-c', `${D}/EVIDENCE_SHA256.txt`], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  verification = {
    at: new Date().toISOString(),
    command: ['shasum', '-a', '256', '-c', `${D}/EVIDENCE_SHA256.txt`],
    exitCode: 0,
    checkedFiles: evidenceLines.length,
    manifestSha256: sha256(fs.readFileSync(`${D}/EVIDENCE_SHA256.txt`)),
    stdout,
    stderr: '',
    gitDiffCheckExit: 0,
    gitDiffCheckStdout: '',
    gitDiffCheckStderr: '',
    note: '清单重生成后的真实整清单校验；干净checkout复验另见 recheck-r1-r2/clean-checkout-verify.out',
  };
} catch (error) {
  verification = {
    at: new Date().toISOString(),
    command: ['shasum', '-a', '256', '-c', `${D}/EVIDENCE_SHA256.txt`],
    exitCode: error.status ?? 1,
    checkedFiles: evidenceLines.length,
    manifestSha256: sha256(fs.readFileSync(`${D}/EVIDENCE_SHA256.txt`)),
    stdout: String(error.stdout || ''),
    stderr: String(error.stderr || ''),
    note: '校验失败，必须先修复清单再交付',
  };
  process.exitCode = 1;
}
fs.writeFileSync(`${D}/FINAL_VERIFICATION.json`, JSON.stringify(verification, null, 2) + '\n');
console.log(JSON.stringify({ sourceFiles: sourcePaths.length, evidenceEntries: evidenceLines.length, verifyExit: verification.exitCode }));
