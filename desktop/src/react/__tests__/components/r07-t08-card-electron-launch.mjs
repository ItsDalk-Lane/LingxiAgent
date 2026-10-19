// 只在独立合成home启动实际窗口，页面导入当前生产InteractiveCard组件。
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../../../..');
const home = process.env.LINGXI_R07_CARD_HOME;
if (!home || !process.env.LINGXI_R07_CARD_URL || !process.env.LINGXI_R07_CARD_BLOCK) throw new Error('只接受真实Rust检查的合成fixture');
delete process.env.VITEST;
const mobile = process.env.LINGXI_R07_CARD_MOBILE_CSP === '1';
const mobileMeta = mobile ? (await fs.promises.readFile(path.join(repo,'desktop/src/mobile.html'),'utf8')).match(/<meta http-equiv="Content-Security-Policy"[^>]+>/)[0] : '';
const server = await createServer({ plugins: [{name:'isolated-original-card-source',enforce:'pre',load(id){ if(process.env.LINGXI_R07_CARD_BASELINE_SOURCE && id.split('?')[0]===path.join(repo,'desktop/src/react/components/chat/InteractiveCard.tsx')) return fs.promises.readFile(process.env.LINGXI_R07_CARD_BASELINE_SOURCE,'utf8'); }} , ...(mobile ? [{name:'actual-mobile-csp',transformIndexHtml(html){return html.replace('<head>','<head>'+mobileMeta);}}] : [])], configFile: path.join(repo, 'vite.config.ts'), root: path.join(repo, 'desktop/src'), cacheDir: path.join(home, '.card-vite-cache'),
  resolve: { alias: { '@': path.join(repo, 'desktop/src/react') } }, esbuild: { jsx: 'automatic' },
  server: { host: '127.0.0.1', port: 0, strictPort: false, fs: { allow: [repo] }, hmr: false, proxy: mobile ? { '/lingxi': { target:process.env.LINGXI_R07_CARD_URL, changeOrigin:true } } : undefined },
  optimizeDeps: { include: ['react', 'react-dom/client', 'zustand'] }, logLevel: 'error' });
try {
  await server.listen();
  const executable = fs.realpathSync(path.join(repo, 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'));
  const renderer = `http://127.0.0.1:${server.httpServer.address().port}/react/__tests__/components/r07-t08-card-fixture.html`;
  const result = await new Promise((resolve, reject) => {
    const child = spawn(executable, [path.join(here, 'r07-t08-card-electron-main.cjs')], { cwd: home,
      env: { ...process.env, LINGXI_R07_CARD_RENDERER: renderer, ELECTRON_ENABLE_LOGGING: '0' }, stdio: ['ignore', 'pipe', 'pipe'] });
    const timeout = setTimeout(() => { child.kill(); reject(new Error('实际卡片窗口超过预算')); }, 50_000);
    child.stdout.on('data', chunk => process.stdout.write(chunk)); child.stderr.on('data', chunk => process.stderr.write(chunk));
    child.once('error', reject); child.once('exit', code => { clearTimeout(timeout); resolve(code); });
  });
  if (result !== 0) throw new Error(`真实卡片窗口失败：${result}`);
} finally { await server.close(); }
