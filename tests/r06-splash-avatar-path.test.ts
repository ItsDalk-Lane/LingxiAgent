import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
const require = createRequire(import.meta.url);
const { resolveCurrentSplashAgentId, readSplashInfo, isTrustedSplashAvatarSender } = require('../desktop/src/shared/splash-avatar-path.cjs');
function home() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-splash-unit-'));
  fs.mkdirSync(path.join(root, 'user'));
  for (const id of ['alpha', 'zeta']) {
    fs.mkdirSync(path.join(root, 'agents', id), { recursive: true });
    fs.writeFileSync(path.join(root, 'agents', id, 'config.yaml'), `agent:\n  name: "${id} 的助手"\n  yuan: "ming"\nlocale: "en"\n`);
  }
  return root;
}
describe('R06 启动画面真实字段与本地路径桥边界', () => {
  it('Rust当前助手优先于默认助手，Node仍读取原字段，读取不更改偏好', () => {
    const root = home(); const prefs = path.join(root, 'user/preferences.json');
    const original = JSON.stringify({ current_agent_id: 'zeta', primary_agent_id: 'alpha', primaryAgent: 'alpha', unknown: 7 });
    fs.writeFileSync(prefs, original);
    expect(resolveCurrentSplashAgentId(root, 'rust')).toBe('zeta');
    expect(resolveCurrentSplashAgentId(root, 'node')).toBe('alpha');
    expect(fs.readFileSync(prefs, 'utf8')).toBe(original);
  });
  it('Rust没有当前记录时读取其真实默认记录', () => {
    const root = home(); fs.writeFileSync(path.join(root, 'user/preferences.json'), JSON.stringify({ primary_agent_id: 'zeta' }));
    expect(resolveCurrentSplashAgentId(root, 'rust')).toBe('zeta');
  });
  it('YAML真实带引号字段正常消费，未知内容保留', () => {
    const root = home(); fs.writeFileSync(path.join(root, 'user/preferences.json'), JSON.stringify({ current_agent_id: 'zeta' }));
    expect(readSplashInfo(root, 'rust')).toEqual({ agentName: 'zeta 的助手', yuan: 'ming', locale: 'en' });
  });
  it('坏类型或坏YAML明确拒绝，错误不包含来源密钥', () => {
    const root = home(); const config = path.join(root, 'agents/alpha/config.yaml');
    fs.writeFileSync(config, 'agent:\n  name: [wrong]\n');
    expect(() => readSplashInfo(root, 'rust')).toThrow('字段类型无效');
    fs.writeFileSync(config, 'agent: [secret-test-value');
    try { readSplashInfo(root, 'rust'); throw new Error('应拒绝'); } catch (error) { expect(String(error)).not.toContain('secret-test-value'); }
  });
  it('越界、超长与不存在选择不读取宅外配置', () => {
    const root = home(); const prefs = path.join(root, 'user/preferences.json');
    for (const id of ['../zeta', 'a/b', 'a\\b', 'x'.repeat(129), 'missing']) {
      fs.writeFileSync(prefs, JSON.stringify({ current_agent_id: id }));
      expect(resolveCurrentSplashAgentId(root, 'rust')).toBe('alpha');
    }
  });
  it('符号链接配置和删除标记不成为启动助手', () => {
    const root = home(); const config = path.join(root, 'agents/alpha/config.yaml');
    fs.renameSync(config, `${config}.held`); fs.symlinkSync(`${config}.held`, config);
    expect(resolveCurrentSplashAgentId(root, 'rust')).toBe('zeta');
    fs.writeFileSync(path.join(root, 'agents/zeta/.deleted-agent.json'), '{}');
    expect(resolveCurrentSplashAgentId(root, 'rust')).toBe(null);
  });
  it('空宅无助手诚实返回默认基本信息', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-splash-empty-'));
    expect(resolveCurrentSplashAgentId(root, 'rust')).toBe(null);
    expect(readSplashInfo(root, 'rust')).toEqual({ agentName: null, locale: 'zh-CN', yuan: 'lingxi' });
  });
  it('只有当前已知窗口的主frame和精确本地页面可获取路径', () => {
    const page = path.join(home(), 'splash.html'); const frame = { url: `${pathToFileURL(page).href}?mode=preparing#ok` };
    const sender = { mainFrame: frame }; const win = { isDestroyed: () => false, webContents: sender };
    expect(isTrustedSplashAvatarSender({ sender, senderFrame: frame }, [win], [page], null)).toBe(true);
    expect(isTrustedSplashAvatarSender({ sender, senderFrame: { ...frame } }, [win], [page], null)).toBe(false);
    expect(isTrustedSplashAvatarSender({ sender, senderFrame: frame }, [], [page], null)).toBe(false);
    frame.url = 'https://example.com/splash.html';
    expect(isTrustedSplashAvatarSender({ sender, senderFrame: frame }, [win], [page], 'https://example.com')).toBe(false);
    frame.url = 'http://127.0.0.1:5173/splash.html';
    expect(isTrustedSplashAvatarSender({ sender, senderFrame: frame }, [win], [page], 'http://127.0.0.1:5173')).toBe(true);
  });
});
