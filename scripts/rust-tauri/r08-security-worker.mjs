// 外围扫描器只接收 Rust 已授权的快照；不接收任务状态或权限裁决权。
import { scanFilesForSecrets, sortFindings, countBySeverity } from '../../lib/security/baseline-scanner.ts';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs/promises';
import path from 'node:path';
const exec = promisify(execFile);
function literal(value) {
  if (typeof value !== 'string' || /[\0\r\n]/.test(value)) throw new Error('SANDBOX_PATH_INVALID');
  return JSON.stringify(value);
}
async function confined(binary, args, input, liveGit = false) {
  // 专用只读边界不复用“允许读取全部文件”的命令沙盒。
  // 软件运行库属于固定平台依赖；用户目录与网络均不开放。
  const readRoots = [...(liveGit ? [input.cwd] : [input.snapshotRoot]), '/usr', '/System', '/Library/Apple', '/opt/homebrew', '/bin', '/sbin'];
  const writeRoots = liveGit ? [] : [input.reportRoot];
  let command, argv;
  if (process.platform === 'darwin') {
    const helper = '/usr/bin/sandbox-exec', stat = await fs.stat(helper);
    if (stat.uid !== 0 || (stat.mode & 0o022)) throw new Error('SCANNER_SANDBOX_UNTRUSTED');
    const profile = ['(version 1)', '(deny default)', '(allow process-exec* process-fork signal)', '(allow sysctl-read)', '(allow mach*)', '(allow ipc-posix*)', '(allow file-read-metadata)',
      '(allow file-read* (literal "/"))', `(allow file-read* (literal ${literal(await fs.realpath(binary))}))`,
      ...readRoots.map(root => `(allow file-read* (subpath ${literal(root)}))`),
      '(allow file-read* (literal "/dev/null") (literal "/dev/urandom") (literal "/dev/random"))',
      '(allow file-write* (literal "/dev/null"))', ...writeRoots.map(root => `(allow file-write* (subpath ${literal(root)}))`), '(deny network*)'].join('\n');
    command = helper; argv = ['-p', profile, '--', binary, ...args];
  } else if (process.platform === 'linux') {
    const helper = '/usr/bin/bwrap', stat = await fs.stat(helper);
    if (stat.uid !== 0 || (stat.mode & 0o022)) throw new Error('SCANNER_SANDBOX_UNTRUSTED');
    argv = ['--unshare-all', '--die-with-parent', '--new-session', '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp'];
    for (const root of [...readRoots, '/lib', '/lib64', '/etc/ld.so.cache', '/etc/ssl']) {
      try { await fs.stat(root); argv.push('--ro-bind', root, root); } catch { /* 平台不存在的运行库目录无需挂载。 */ }
    }
    for (const root of writeRoots) argv.push('--bind', root, root);
    argv.push('--chdir', liveGit ? input.cwd : input.snapshotRoot, '--', binary, ...args); command = helper;
  } else throw new Error('SCANNER_SANDBOX_UNSUPPORTED');
  return exec(command, argv, { cwd: liveGit ? input.cwd : input.snapshotRoot, timeout: liveGit ? 15000 : 10000, maxBuffer: 8 * 1024 * 1024,
    env: { ...process.env, HOME: liveGit ? input.cwd : input.reportRoot, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: process.platform === 'win32' ? 'NUL' : '/dev/null' } });
}
let raw = '';
for await (const chunk of process.stdin) {
  raw += chunk;
  if (Buffer.byteLength(raw) > 40 * 1024 * 1024) throw new Error('SECURITY_INPUT_LIMIT');
}
try {
  const input = JSON.parse(raw);
  if (input.op === 'changes') {
    try {
      const { stdout } = await confined(input.git, ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-c', 'core.untrackedCache=false', 'status', '--porcelain=v1', '-z', '--untracked-files=all'], input, true);
      const entries = stdout.split('\0'), files = [];
      for (let i = 0; i < entries.length; i++) {
        const entry = entries[i];
        if (!entry) continue;
        if (entry.length < 4 || entry[2] !== ' ') throw new Error('GIT_STATUS_INVALID');
        files.push(entry.slice(3));
        if (/[RC]/.test(entry.slice(0, 2))) i++; // -z 重命名的下一项是旧路径。
      }
      process.stdout.write(JSON.stringify({ files }));
    } catch (error) {
      process.stdout.write(JSON.stringify({ files: [], note: String(error.stderr || '').includes('not a git repository') ? 'not_a_git_repo: use mode=path' : 'git_unavailable: use mode=path' }));
    }
  } else {
    const baseline = scanFilesForSecrets(input.files);
    const findings = [...baseline.findings], engines = ['baseline'], external = {};
    for (const name of ['semgrep', 'gitleaks']) {
      if (!input.engines?.[name]) { external[name] = { status: 'missing' }; continue; }
      try {
        if (name === 'semgrep') {
          const { stdout } = await confined(input.engines[name], ['scan', '--json', '--quiet', '--timeout', '8', '.'], input);
          const result = JSON.parse(stdout);
          if (!Array.isArray(result.results) || (result.errors?.length ?? 0)) throw new Error('SEMGREP_INCOMPLETE');
          for (const hit of result.results) findings.push({ file: hit.path, line: hit.start?.line, ruleId: `semgrep:${hit.check_id}`, severity: 'warning' });
        } else {
          const report = path.join(input.reportRoot, 'gitleaks.json');
          await confined(input.engines[name], ['detect', '--no-git', '--source', input.snapshotRoot, '--report-format', 'json', '--report-path', report, '--redact', '--no-banner', '--exit-code', '0'], input);
          const result = JSON.parse(await fs.readFile(report, 'utf8'));
          if (!Array.isArray(result)) throw new Error('GITLEAKS_INVALID');
          for (const hit of result) findings.push({ file: path.isAbsolute(hit.File) ? path.relative(input.snapshotRoot, hit.File) : hit.File, line: hit.StartLine, ruleId: `gitleaks:${hit.RuleID}`, severity: 'critical' });
        }
        engines.push(name); external[name] = { status: 'completed' };
      } catch {
        // 不回传 stderr，第三方诊断可能包含命中正文；失败不能冒充无命中。
        external[name] = { status: 'failed', code: 'SCANNER_EXECUTION_FAILED' };
      }
    }
    const allowed = new Set(input.files.map(file => file.path));
    const seen = new Set();
    const safe = sortFindings(findings.filter(hit => {
      const key = `${hit.file}:${hit.line}:${hit.ruleId}`;
      if (!allowed.has(hit.file) || !Number.isSafeInteger(hit.line) || hit.line < 1 || seen.has(key)) return false;
      seen.add(key); return true;
    }).map(({ file, line, ruleId, severity }) => ({ file, line, ruleId, severity })));
    process.stdout.write(JSON.stringify({ ...baseline, findings: safe.slice(0, 50), findingCount: safe.length, counts: countBySeverity(safe), engines, external }));
  }
} catch {
  process.stdout.write(JSON.stringify({ error: 'SECURITY_WORKER_FAILED' }));
  process.exitCode = 1;
}
