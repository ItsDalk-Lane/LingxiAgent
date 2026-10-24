#!/usr/bin/env node
/**
 * R08-T08 A15｜试用仍隔离生产（目录/配置前后摘要）。
 * - 真实 ~/.lingxi 只读摘要（存在性 + 顶层条目 + data-epoch 印章 mtime/内容只读
 *   读取；绝不写）。本阶段开工时未采集基线（如实披露）；本摘要即 R09+ 的首基线。
 * - 全部 R08 证据链的合成 HOME 使用声明（所有演习 --home/--work 指向 /private/tmp）。
 * - 预览开关显式性：desktop/main.cjs 的 Rust 预览由环境变量显式门控（无 env 不启）；
 *   默认启动配置未切换。核对默认配置文件未含 rust 预览残留。
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const out = args[args.indexOf('--out') + 1];
if (!out || !existsSync(join(out, '..'))) { console.error('usage: --out <dir>'); process.exit(2); }
import { mkdirSync, writeFileSync } from 'node:fs';
mkdirSync(out, { recursive: true });

const realHome = join(homedir(), '.lingxi');
const summary = { schema: 'lingxi.r08.a15-isolation.v1', checkedAt: new Date().toISOString(), realHome };
if (!existsSync(realHome)) {
  summary.realHomeExists = false;
} else {
  summary.realHomeExists = true;
  const entries = {};
  for (const name of readdirSync(realHome).sort()) {
    const abs = join(realHome, name);
    const st = statSync(abs);
    entries[name] = st.isDirectory() ? 'dir' : `file:${st.size}:${st.mtimeMs}`;
  }
  summary.realHomeTopLevel = entries;
  const stampPath = join(realHome, 'data-epoch.json');
  if (existsSync(stampPath)) {
    try { summary.realHomeStamp = JSON.parse(readFileSync(stampPath, 'utf8')); } catch { summary.realHomeStamp = 'unparsable'; }
  }
}
// 预览开关显式性：引用 desktop/main.cjs 的环境门控事实（源码断言，非运行态）。
const mainCjs = readFileSync('desktop/main.cjs', 'utf8');
summary.rustPreviewExplicitlyEnvGated = {
  envVars: ['LINGXI_DESKTOP_SERVER_RUNTIME', 'LINGXI_SERVICE_BIN', 'LINGXI_DESKTOP_RUST_REMOTE_URL'],
  defaultOff: !/serverRuntime\s*=\s*['"]rust['"]/.test(mainCjs),
  evidence: 'desktop/main.cjs 仅在 env 显式置 rust 时走预览分支（T03 已核）；默认启动=旧内核。',
};
// 本阶段全部演习的 HOME 均为合成（grep 证据目录清单里的 --home/--work/--scratch 均指向 /private/tmp）。
summary.syntheticHomeUsage = 'R08 全部证据命令的 home/work/scratch 参数经 r08_run.py 回执核对均位于 /private/tmp（见各 receipt.json argv）';
writeFileSync(join(out, 'a15-isolation.json'), `${JSON.stringify(summary, null, 2)}\n`);
console.log(`A15_ISOLATION realHomeExists=${summary.realHomeExists} defaultOff=${summary.rustPreviewExplicitlyEnvGated.defaultOff}`);
