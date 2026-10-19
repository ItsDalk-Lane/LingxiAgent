// 严格单例包装：实际 Electron 当前设置入口 + 真服务，不替换 fetch、图片、Canvas 或存储。
import { test, expect } from 'vitest';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

const enabled = !!process.env.R06_SETTINGS_URL;
test.skipIf(!enabled)('rust_agent_settings_current_electron_real_service', async () => {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../../..');
  const script = path.join(root, 'desktop/src/react/settings/__tests__/r06-settings-electron-launch.mjs');
  const result = await new Promise<{ code: number | null; stdout: string; stderr: string }>((resolve, reject) => {
    const child = spawn(process.env.R06_NODE_BIN || process.execPath, [script], { cwd: process.env.R06_SETTINGS_HOME, env: process.env, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = ''; let stderr = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.stderr.on('data', chunk => { stderr += chunk; });
    child.on('error', reject);
    child.on('exit', code => resolve({ code, stdout, stderr }));
  });
  expect(result.code, result.stderr + result.stdout).toBe(0);
  const reports = result.stdout.split('\n').filter(line => line.startsWith('R06_SETTINGS_ELECTRON_RESULT:'));
  expect(reports).toHaveLength(1);
  const report = JSON.parse(reports[0].slice('R06_SETTINGS_ELECTRON_RESULT:'.length));
  expect(report.scenario).toBe(process.env.R06_SETTINGS_CASE || 'smoke');
  if (process.env.R06_SETTINGS_CASE === 'read-denied') expect(report.checks).toEqual(['denied-ready-unmasked', 'denied-explicit-error-state', 'denied-actual-403', 'denied-zero-mutations']);
  else expect(report.checks.slice(0, 6)).toEqual(['production-settings-entry', 'actual-agents-list', 'actual-settings-snapshot', 'actual-model-catalog', 'actual-current-model-binding', 'explicit-maintenance-state']);
  if (process.env.R06_SETTINGS_CASE === 'workflow') {
    for (let action = 0; action <= 18; action++) expect(report.checks.some((name: string) => name.startsWith(String(action).padStart(2, '0') + '-'))).toBe(true);
    expect(report.checks).toContain('agent-B-isolated');
  }
  if (process.env.R06_SETTINGS_CASE === 'references-pending') expect(report.checks.slice(6)).toEqual(['reference-pending-real-receipt', 'reference-pending-agent-absent', 'reference-read-failure-keeps-pending-visible', 'reference-pending-no-false-switch', 'reference-retry-completed', 'reference-retry-actual-prefs']);
  if (process.env.R06_SETTINGS_CASE === 'card-owner') expect(report.checks.slice(6)).toEqual(['card-owner-export-preview-hidden-on-B','card-owner-export-away-back-invalid','card-owner-import-preview-hidden-on-B','card-owner-import-away-back-invalid','card-owner-import-completed-only-on-A','card-owner-no-import-or-switch-to-B','card-owner-no-old-created-projection-on-B','card-owner-completed-response-stays-invalid-after-return']);
  if (process.env.R06_SETTINGS_CASE === 'initialization') expect(report.checks.slice(6)).toEqual(['00-slow-summary-does-not-block-ready','00-published-summary-survives-legacy-corruption','00-model-read-failure-explicit-not-empty-success','00-same-owner-cache-reopen-no-mask','00-cold-mask-before-forced-ready','00-real-fifteen-second-forced-ready','00-forced-ready-does-not-fake-loaded-data']);
  if (process.env.R06_SETTINGS_CASE === 'boundaries') expect(report.checks.slice(6)).toEqual(['14-write-persisted-before-refresh-error','14-success-post-failed-get-old-list-and-error','14-readback-confirms-no-fake-rollback','17-disabled-clears-old-health-and-zero-get','17-actual-read-error-unavailable','17-disabled-clears-error-and-zero-get','18-native-trusted-pointercancel-zero-order-put','18-pointercancel-keeps-real-order','18-order-real-io-failure-then-get-recovery','18-order-failure-preserves-real-prefs']);
  if (process.env.R06_SETTINGS_CASE === 'receipt-owner') expect(report.checks.slice(6)).toEqual(['receipt-owner-A-visible','receipt-owner-B-loading-hides-A','receipt-owner-B-zero-cleanup','receipt-owner-return-A-reloaded','receipt-owner-late-B-cannot-replace-A']);
  if (process.env.R06_SETTINGS_CASE === 'failures') expect(report.checks.slice(6)).toEqual(['memory-io-failure-preserves-config', 'embedding-io-failure-no-false-save', 'dream-auto-io-failure-preserved', 'mixed-patch-after-global-before-agent-fails', 'mixed-patch-real-global-persisted-agent-not', 'config-get-failure-does-not-imply-rollback', 'config-put-success-get-failure-visible', 'persona-real-one-success-one-failure', 'persona-partial-write-honest', 'compiled-read-error-is-not-empty-success', 'compiled-failure-preserved-material', 'models-failure-keeps-existing-list']);
  fs.writeFileSync(path.join(process.env.R06_SETTINGS_HOME!, 'electron-result.json'), JSON.stringify(report, null, 2));
}, 150_000);
