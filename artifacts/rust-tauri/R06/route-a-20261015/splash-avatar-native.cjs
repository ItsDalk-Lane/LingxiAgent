// 真实Rust保存后停服，使用现役IPC注册、preload及SplashApp观察本地窗口；仅使用合成资料。
const { app, BrowserWindow, ipcMain, nativeImage } = require('electron');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const assert = require('assert/strict');
const { spawn } = require('child_process');
const { once } = require('events');
const { pathToFileURL } = require('url');
const { PNG } = require('pngjs');
const root = path.resolve(process.argv[2]);
const output = path.resolve(process.argv[3]);
const binary = path.resolve(process.argv[4]);
fs.mkdirSync(output, { recursive: true });
app.setPath('userData', path.join(output, 'electron-profile'));
app.on('window-all-closed', () => {});
const { resolveLingxiHome } = require(path.join(root, 'shared/hana-runtime-paths.cjs'));
const { rustDesktopEnabled, resolveRustBinary, readOwnedRustConnection } = require(path.join(root, 'desktop/src/shared/rust-local-service.cjs'));
const { registerSplashDataIpc, isTrustedSplashAvatarSender } = require(path.join(root, 'desktop/src/shared/splash-avatar-path.cjs'));
const home = resolveLingxiHome(path.join(output, 'home with space #角色'));
const splash = path.join(root, 'desktop/dist-splash/splash.html');
let service; let win; let extra; let currentHome = home;
const records = [];
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function image(r, g, b) {
  const png = new PNG({ width: 48, height: 48 });
  for (let n = 0; n < png.data.length; n += 4) png.data.set([r, g, b, 255], n);
  return PNG.sync.write(png);
}
async function startRust() {
  process.env.LINGXI_DESKTOP_SERVER_RUNTIME = 'rust';
  process.env.LINGXI_HOME = home;
  process.env.LINGXI_SERVICE_BIN = binary;
  assert.equal(rustDesktopEnabled(), true);
  const selected = resolveRustBinary({ root, packaged: false, env: process.env });
  assert.equal(selected, binary);
  service = spawn(selected, ['--home', home, '--bind', '127.0.0.1:0'], {
    cwd: path.dirname(selected), env: { ...process.env, LINGXI_HOME: home, LINGXI_PRODUCT_DIR: path.join(root, 'lib') }, stdio: ['ignore', 'pipe', 'pipe'],
  });
  const stderr = fs.createWriteStream(path.join(output, 'rust-stderr.log')); service.stderr.pipe(stderr);
  let all = '';
  const addr = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Rust未真实就绪')), 30000);
    service.on('exit', (code, signal) => { clearTimeout(timer); reject(new Error(`Rust提前退出 ${code}/${signal}`)); });
    service.stdout.on('data', bytes => {
      all += bytes.toString(); const ready = all.match(/LINGXI_SERVICE_READY .*addr=([^\s]+)/);
      if (ready) { clearTimeout(timer); resolve(ready[1]); }
    });
  });
  const connection = readOwnedRustConnection({ home, pid: service.pid, readyAddr: addr });
  assert.ok(connection && connection.token);
  const request = async (method, suffix, body) => {
    const res = await fetch(`http://${addr}/lingxi/v1${suffix}`, { method, headers: { Authorization: `Bearer ${connection.token}`, 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
    const value = await res.json(); assert.equal(res.status, 200, `${method} ${suffix}: ${JSON.stringify(value)}`); return value;
  };
  await request('POST', '/agents', { id: 'alpha', name: '第一位助手', yuan: 'butter' });
  await request('POST', '/agents', { id: 'zeta', name: '当前助手', yuan: 'ming' });
  await request('PUT', '/agents/primary', { id: 'alpha' });
  await request('POST', '/agents/switch', { id: 'zeta' });
  const encoder = new BrowserWindow({ show: false, webPreferences: { contextIsolation: true, nodeIntegration: false } });
  await encoder.loadURL('about:blank');
  const encoded = await encoder.webContents.executeJavaScript(`(() => {const c=document.createElement('canvas');c.width=c.height=48;const ctx=c.getContext('2d');ctx.fillStyle='rgb(24,140,236)';ctx.fillRect(0,0,48,48);return c.toDataURL('image/webp');})()`);
  encoder.destroy(); assert.ok(encoded.startsWith('data:image/webp;base64,'));
  const bytes = Buffer.from(encoded.split(',')[1], 'base64'); const other = nativeImage.createFromBuffer(image(100, 210, 32)).toJPEG(90); const user = image(190, 60, 120);
  for (const [suffix, saved, mime] of [['/agents/alpha/avatar', other, 'image/jpeg'], ['/agents/zeta/avatar', bytes, 'image/webp'], ['/avatar/user', user, 'image/png']]) await request('POST', suffix, { data: `data:${mime};base64,${saved.toString('base64')}` });
  const prefs = JSON.parse(fs.readFileSync(path.join(home, 'user/preferences.json')));
  assert.equal(prefs.current_agent_id, 'zeta'); assert.equal(prefs.primary_agent_id, 'alpha');
  const avatar = path.join(home, 'agents/zeta/avatars/agent.webp');
  const userAvatar = path.join(home, 'user/avatars/user.png');
  assert.equal(sha(fs.readFileSync(avatar)), sha(bytes)); assert.equal(sha(fs.readFileSync(userAvatar)), sha(user));
  records.push({ case: 'real-rust-write-and-focus', runtime: 'rust', home, agent: 'zeta', primary: 'alpha', avatar, sha256: sha(bytes), userAvatar, binarySha256: sha(fs.readFileSync(binary)) });
  const exited = once(service, 'exit'); service.kill('SIGTERM'); await exited;
  assert.equal(service.exitCode, 0); service = null;
  await assert.rejects(fetch(`http://${addr}/lingxi/v1/health`));
  records.push({ case: 'actual-service-stopped-before-splash', serviceOffline: true });
  return { avatar, userAvatar, bytes, user };
}
function register() {
  ipcMain.removeHandler('get-avatar-path'); ipcMain.removeHandler('get-splash-info');
  if (process.env.LINGXI_SPLASH_BASELINE_SOURCE) {
    // 负控只装入精确旧宿主函数/注册片段，IPC与窗口仍真实；不提供返回字符串替身。
    const source = fs.readFileSync(process.env.LINGXI_SPLASH_BASELINE_SOURCE, 'utf8');
    const functionStart = source.indexOf('function getCurrentAgentId() {');
    const functionEnd = source.indexOf('\n}\n', functionStart) + 2;
    const handlerStart = source.indexOf('// 获取头像本地路径（splash 用，不依赖 server）');
    const handlerEnd = source.indexOf('// 选择文件夹', handlerStart);
    require('vm').runInNewContext(source.slice(functionStart,functionEnd) + '\n' + source.slice(handlerStart,handlerEnd), { fs, path, lingxiHome: currentHome, wrapIpcHandler: require(path.join(root,'desktop/ipc-wrapper.cjs')).wrapIpcHandler });
    return;
  }
  registerSplashDataIpc({ home: currentHome, senderAllowed: event => isTrustedSplashAvatarSender(event, [win], [splash], null) });
}
async function waitImage(expected) {
  const deadline = Date.now() + 10000;
  let value;
  while (Date.now() < deadline) {
    value = await win.webContents.executeJavaScript(`(() => { const i=document.querySelector('.splash-avatar'); return i && { src:i.src,currentSrc:i.currentSrc,complete:i.complete,width:i.naturalWidth,height:i.naturalHeight,text:document.querySelector('.splash-text')?.textContent,opacity:Number(getComputedStyle(document.body).opacity) }; })()`);
    if (value && value.complete && value.width > 0 && value.opacity >= 0.99 && value.src.includes(expected)) return value;
    await new Promise(resolve => setTimeout(resolve, 30));
  }
  throw new Error(`当前SplashApp没有真实显示预期图片 ${expected}: ${JSON.stringify(value)}`);
}
async function view(caseId, expected, screenshot = false) {
  await win.loadFile(splash); const shown = await waitImage(expected);
  assert.equal(win.isVisible(), true);
  const result = await win.webContents.executeJavaScript(`Promise.all([window.hana.getAvatarPath('agent'),window.hana.getSplashInfo()])`);
  records.push({ case: caseId, visible: true, renderer: shown, localPath: result[0], splashInfo: result[1] });
  if (screenshot) fs.writeFileSync(path.join(output, `${caseId}.png`), (await win.webContents.capturePage()).toPNG());
  return result;
}
app.whenReady().then(async () => {
  const saved = await startRust();
  win = new BrowserWindow({ width: 380, height: 280, show: true, frame: false, resizable: false, webPreferences: { preload: path.join(root, 'desktop/preload.bundle.cjs'), contextIsolation: true, nodeIntegration: false } });
  ipcMain.handle('get-platform', () => process.platform);
  register();
  const focused = await view('rust-saved-offline-current-avatar', '/agents/zeta/avatars/agent.webp', true);
  assert.equal(focused[0], saved.avatar); assert.equal(focused[1].yuan, 'ming');
  const userResult = await win.webContents.executeJavaScript(`window.hana.getAvatarPath('user')`); assert.equal(userResult, saved.userAvatar);
  const rejected = await win.webContents.executeJavaScript(`Promise.all([window.hana.getAvatarPath('../agent'),window.hana.getAvatarPath({role:'agent'}),window.hana.getAvatarPath('other')])`); assert.deepEqual(rejected, [null, null, null]);
  const userDecoded = await win.webContents.executeJavaScript(`(async () => { const p=await window.hana.getAvatarPath('user'); return new Promise((resolve,reject)=>{ const image=new Image(); image.onload=()=>resolve({width:image.naturalWidth,height:image.naturalHeight}); image.onerror=()=>reject(new Error('用户头像真实加载失败')); image.src=window.platform.getFileUrl(p); }); })()`);
  assert.deepEqual(userDecoded,{width:48,height:48});
  records.push({ case: 'user-and-invalid-role-real-ipc', userPath: userResult, userDecoded, rejected });
  extra = new BrowserWindow({ show: false, webPreferences: { preload: path.join(root, 'desktop/preload.bundle.cjs'), contextIsolation: true, nodeIntegration: false } });
  await extra.loadFile(splash);
  assert.equal(await extra.webContents.executeJavaScript(`window.hana.getAvatarPath('agent')`), null);
  extra.close(); extra = null;
  await win.loadURL('data:text/html,<html><body>untrusted origin</body></html>');
  assert.equal(await win.webContents.executeJavaScript(`window.hana.getAvatarPath('agent')`), null);
  records.push({ case: 'unknown-window-and-untrusted-origin-refused', localPath: null });
  const held = `${saved.avatar}.test-held`;
  fs.renameSync(saved.avatar, held);
  assert.equal((await view('missing-default', '/assets/Ming.png', true))[0], null);
  for (const [id, invalid] of [['wrong-format-default', Buffer.from('not an image')], ['malformed-webp-default', Buffer.from('52494646080000005745425000000000', 'hex')], ['extension-mismatch-default', nativeImage.createFromBuffer(saved.user).toJPEG(90)]]) {
    fs.writeFileSync(saved.avatar, invalid); assert.equal((await view(id, '/assets/Ming.png'))[0], null); fs.unlinkSync(saved.avatar);
  }
  fs.mkdirSync(saved.avatar);
  assert.equal((await view('nonregular-directory-default', '/assets/Ming.png'))[0], null); fs.rmdirSync(saved.avatar);
  require('child_process').execFileSync('mkfifo', [saved.avatar]);
  assert.equal((await view('nonregular-fifo-default', '/assets/Ming.png'))[0], null); fs.unlinkSync(saved.avatar);
  const outside = path.join(output, 'outside.png'); fs.writeFileSync(outside, saved.bytes); fs.symlinkSync(outside, saved.avatar);
  assert.equal((await view('symlink-file-default', '/assets/Ming.png'))[0], null); fs.unlinkSync(saved.avatar);
  fs.renameSync(held, saved.avatar);
  const dir = path.dirname(saved.avatar); const heldDir = `${dir}.test-held`; const externalDir = path.join(output, 'external-avatars'); fs.mkdirSync(externalDir); fs.writeFileSync(path.join(externalDir, 'agent.webp'), saved.bytes);
  fs.renameSync(dir, heldDir); fs.symlinkSync(externalDir, dir);
  assert.equal((await view('symlink-ancestor-default', '/assets/Ming.png'))[0], null); fs.unlinkSync(dir); fs.renameSync(heldDir, dir);
  fs.chmodSync(saved.avatar, 0);
  await win.loadFile(splash); const permissionDefault = await waitImage('/assets/Ming.png');
  const permissionReject = await win.webContents.executeJavaScript(`window.hana.getAvatarPath('agent').then(()=>false,()=>true)`); assert.equal(permissionReject, true);
  records.push({ case: 'actual-permission-rejection-default', renderer: permissionDefault, ipcRejected: permissionReject }); fs.chmodSync(saved.avatar, 0o600);
  let removedAfterPath = false;
  win.webContents.session.webRequest.onBeforeRequest({ urls: ['file:///*'] }, (details, callback) => {
    if (details.url.startsWith(pathToFileURL(saved.avatar).href) && !removedAfterPath) { fs.renameSync(saved.avatar, held); removedAfterPath = true; }
    callback({});
  });
  await win.loadFile(splash); const loadFailed = await waitImage('/assets/Ming.png');
  assert.equal(removedAfterPath, true, '必须在真实IPC返回后的Chromium文件加载前移走原文件');
  records.push({ case: 'actual-file-load-failure-default', renderer: loadFailed, fileRemovedAfterPath: true });
  win.webContents.session.webRequest.onBeforeRequest(null); fs.renameSync(held, saved.avatar);
  assert.equal(sha(fs.readFileSync(saved.avatar)), sha(saved.bytes)); assert.equal(sha(fs.readFileSync(saved.userAvatar)), sha(saved.user));
  const empty = path.join(output, 'empty-home'); fs.mkdirSync(empty); currentHome = empty; register();
  const noAgent = await view('offline-empty-default', '/assets/Lingxi.png', true); assert.equal(noAgent[0], null); assert.equal(noAgent[1].agentName, null);
  fs.writeFileSync(path.join(output, 'receipt.json'), JSON.stringify({ leaf: 'R00-T02-LA-A02D8C572F5C', result: 'PASS', backend: 'rust', serverRequiredForSplash: false, actualElectron: process.versions.electron, assertions: ['返回本地头像路径供 splash 渲染','路径不存在回退默认头像；不等于设置或更新头像'], records }, null, 2));
  console.log(`SPLASH_AVATAR_NATIVE_PASS cases=${records.length} receipt=${path.join(output,'receipt.json')}`);
}).catch(error => { console.error(error); process.exitCode = 1; }).finally(async () => {
  if (service) service.kill('SIGKILL'); if (extra && !extra.isDestroyed()) extra.destroy(); if (win && !win.isDestroyed()) win.destroy(); app.exit(process.exitCode || 0);
});
