/** @vitest-environment node */
// 由真实Rust Run检查传入服务地址和工具结果；没有环境变量只跳过，不能当通过。
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const enabled = process.platform === 'darwin' && !!process.env.LINGXI_R07_CARD_URL;
describe('T08 真实Run到实际互动卡片窗口', () => {
  it.skipIf(!enabled)('真实组件登记、实际iframe按钮响应、CSP阻止请求并回收窗口', async () => {
    const script = fileURLToPath(new URL('./r07-t08-card-electron-launch.mjs', import.meta.url));
    const report = await new Promise<{ code: number | null; stdout: string; stderr: string }>((resolve, reject) => {
      const child = spawn(process.execPath, [script], { env: process.env, stdio: ['ignore', 'pipe', 'pipe'] });
      let stdout = ''; let stderr = '';
      const timeout = setTimeout(() => { child.kill(); reject(new Error(`真实卡片窗口超时：${stderr.slice(-3000)}`)); }, 60_000);
      child.stdout.on('data', chunk => { stdout += String(chunk); });
      child.stderr.on('data', chunk => { stderr += String(chunk); });
      child.once('error', reject);
      child.once('exit', code => { clearTimeout(timeout); resolve({ code, stdout, stderr }); });
    });
    expect(report.code, report.stderr).toBe(0);
    expect(report.stdout).toContain('T08_CARD_NATIVE_RESULT:');
    expect(report.stdout).toContain('actual-frame-click');
    expect(report.stdout).toContain('actual-csp-connect-blocked');
  }, 75_000);
});
