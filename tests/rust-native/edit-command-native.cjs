// 当前Rust服务、生产IPC注册正文和实际preload；只在合成输入框中观察系统编辑事件。
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { once } = require('node:events');
const repo = path.resolve(process.argv[2]);
const output = path.resolve(process.argv[3]);
const binary = path.resolve(process.argv[4]);
fs.mkdirSync(output, { recursive: true });
app.setPath('userData', path.join(output, 'electron-profile'));
app.commandLine.appendSwitch('disable-gpu');
app.on('window-all-closed', () => {});
const digest = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
let service;
let win;
let other;

async function startRust() {
  const home = path.join(output, 'home');
  service = spawn(binary, ['--home', home, '--bind', '127.0.0.1:0'], {
    env: { ...process.env, LINGXI_HOME: home, LINGXI_PRODUCT_DIR: path.join(output, 'product') },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const errors = [];
  service.stderr.on('data', bytes => errors.push(bytes));
  let text = '';
  const address = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('当前Rust服务未就绪')), 30000);
    service.once('exit', code => { clearTimeout(timer); reject(new Error(`Rust提前退出 ${code}`)); });
    service.stdout.on('data', bytes => {
      text += bytes;
      const ready = text.match(/LINGXI_SERVICE_READY.*addr=(\S+)/);
      if (ready) { clearTimeout(timer); resolve(ready[1]); }
    });
  });
  const { readOwnedRustConnection } = require(path.join(repo, 'desktop/src/shared/rust-local-service.cjs'));
  const connection = readOwnedRustConnection({ home, pid: service.pid, readyAddr: address });
  assert.ok(connection?.token, '必须是实际当前Rust子进程的连接');
  const response = await fetch(`http://${address}/lingxi/v1/server/identity`, {
    headers: { Authorization: `Bearer ${connection.token}` },
  });
  assert.equal(response.status, 200);
  assert.equal((await response.json()).serverNodeKind, 'lingxi-service');
  const source = fs.readFileSync(path.join(repo, 'desktop/main.cjs'), 'utf8');
  const begin = source.indexOf('wrapIpcHandler("get-server-port",');
  const end = source.indexOf('\n// 壳身份用途', begin);
  assert.ok(begin > 0 && end > begin, '必须加载当前生产注册正文');
  vm.runInNewContext(source.slice(begin, end), {
    serverPort: Number(address.split(':').at(-1)), serverToken: connection.token,
    serverNodeKind: 'lingxi-service', serverNodeTransport: 'http',
    wrapIpcHandler: require(path.join(repo, 'desktop/ipc-wrapper.cjs')).wrapIpcHandler,
  });
  return { address, errors };
}

async function stopRust() {
  if (service && service.exitCode === null) {
    const stopped = once(service, 'exit');
    service.kill('SIGTERM');
    await stopped;
    assert.equal(service.exitCode, 0, '当前Rust服务必须真实正常停止');
  }
}

app.whenReady().then(async () => {
  try {
    const binarySha256 = digest(binary);
    const rust = await startRust();
    const html = path.join(output, 'editor.html');
    // 取消默认剪贴板行为，只观察原生事件，不读取或改写用户的系统剪贴板。
    fs.writeFileSync(html, '<!doctype html><meta charset="utf-8"><input value="合成编辑样例"><script>window.editEvents=[];for(const name of ["cut","copy","paste"])document.addEventListener(name,event=>{event.preventDefault();window.editEvents.push(event.type)});</script>');
    const options = { webPreferences: { preload: path.join(repo, 'desktop/preload.bundle.cjs'), contextIsolation: true, nodeIntegration: false } };
    win = new BrowserWindow({ ...options, show: true, width: 400, height: 200 });
    other = new BrowserWindow({ ...options, show: false });
    await Promise.all([win.loadFile(html), other.loadFile(html)]);
    win.focus();
    const js = source => win.webContents.executeJavaScript(source, true);
    const connection = await js('window.hana.getServerConnectionInfo()');
    assert.equal(connection.serverNodeKind, 'lingxi-service');
    assert.equal(connection.port, Number(rust.address.split(':').at(-1)));
    await js('document.querySelector("input").focus();document.querySelector("input").select()');
    const allowed = [];
    for (const command of ['cut', 'copy', 'paste']) {
      const before = await js('editEvents.length');
      assert.equal(await js(`window.hana.runEditCommand(${JSON.stringify(command)})`), true);
      for (let i = 0; i < 50 && await js('editEvents.length') === before; i++) await wait(20);
      const events = await js('editEvents');
      assert.equal(events[before], command, '必须收到真实窗口原生编辑事件');
      assert.equal(await js('document.querySelector("input").value'), '合成编辑样例');
      allowed.push({ command, actualNativeEvent: events[before] });
    }
    await js('document.querySelector("input").setSelectionRange(2,2)');
    assert.equal(await js('window.hana.runEditCommand("selectAll")'), true);
    await wait(80);
    assert.deepEqual(await js('({start:document.querySelector("input").selectionStart,end:document.querySelector("input").selectionEnd})'), { start: 0, end: 6 });
    allowed.push({ command: 'selectAll', actualSelection: true });
    const refused = ['shell', 'undo', 'executeJavaScript', '__proto__', null, 42, {}, ['copy'], {toString: 'copy'}, {valueOf: 'paste'}];
    for (const command of refused) {
      assert.match(await js(`window.hana.runEditCommand(${JSON.stringify(command)}).then(()=>"UNEXPECTED_SUCCESS",error=>String(error))`), /Unknown edit command/);
    }
    assert.equal((await js('editEvents')).length, 3, '非法参数不触发额外原生事件');
    assert.equal(win.webContents.isDevToolsOpened(), false);
    assert.deepEqual(await other.webContents.executeJavaScript('editEvents'), [], '不能向另一个窗口派发');
    assert.equal(digest(binary), binarySha256);
    win.destroy(); other.destroy();
    await stopRust();
    fs.writeFileSync(path.join(output, 'rust-stderr.log'), Buffer.concat(rust.errors));
    const receipt = { status: 'PASS_MAC_ONLY', binarySha256, allowed, refusedCount: refused.length, otherWindowEvents: 0, fixtureReadsClipboardContents: false, productionMainSha256: digest(path.join(repo, 'desktop/main.cjs')), productionPreloadSourceSha256: digest(path.join(repo, 'desktop/preload.cjs')), actualPreloadBundleSha256: digest(path.join(repo, 'desktop/preload.bundle.cjs')) };
    fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(receipt, null, 2));
    console.log('R06_EDIT_COMMAND_NATIVE_PASS');
    app.exit(0);
  } catch (error) {
    console.error(error.stack || String(error));
    win?.destroy(); other?.destroy(); service?.kill('SIGTERM');
    app.exit(1);
  }
});
