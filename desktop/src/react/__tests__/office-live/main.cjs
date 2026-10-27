// 当前Rust服务、生产IPC注册正文和实际preload；只在合成输入框中观察系统编辑事件。
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
require('electron').ipcMain.handle('get-platform',()=>process.platform);
const { once } = require('node:events');
const repo = path.resolve(__dirname,'../../../../..');
const output = path.resolve(process.env.R06_OFFICE_HOME);
const binary = path.resolve(process.env.R06_OFFICE_BIN);
fs.mkdirSync(output, { recursive: true });
app.setPath('userData', path.join(output, 'electron-profile'));
app.commandLine.appendSwitch('disable-gpu');
app.on('window-all-closed', () => {});
const digest = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const requests=[];
let service;
let win;
let other;

async function startRust() {
  const home = path.join(output, 'home');
  service = spawn(binary, ['--home', home, '--bind', '127.0.0.1:0'], {
    env: { ...process.env, LINGXI_HOME: home, LINGXI_PRODUCT_DIR: path.join(repo, 'lib') },
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
  const beginOffice=source.indexOf('async function readRustOfficePreview(');
  const endOffice=source.indexOf('// 重新加载主窗口',beginOffice);
  assert.ok(beginOffice>0&&endOffice>beginOffice);
  vm.runInNewContext(source.slice(beginOffice,endOffice),{
    serverPort:Number(address.split(':').at(-1)),serverToken:connection.token,serverNodeKind:'lingxi-service',
    wrapIpcHandler:require(path.join(repo,'desktop/ipc-wrapper.cjs')).wrapIpcHandler,
    path,fs,require,AbortSignal,encodeURIComponent,
    fetch:async(...args)=>{const row={url:String(args[0])};requests.push(row);const response=await fetch(...args);row.status=response.status;return response},
  });
  return { address, errors, home };
}

async function stopRust() {
  if (service && service.exitCode === null) {
    const stopped = once(service, 'exit');
    service.kill('SIGTERM');
    await stopped;
    assert.equal(service.exitCode, 0, '当前Rust服务必须真实正常停止');
  }
}


app.whenReady().then(async()=>{
  try{
    const before=digest(binary);const rust=await startRust();
    const documents=path.join(rust.home,'documents');fs.mkdirSync(documents);
    const fixture=path.join(output,'fixtures');
    for(const name of ['actual.docx','actual.xlsx','actual-dates.xlsx','broken.docx','broken.xlsx'])fs.copyFileSync(path.join(fixture,name),path.join(documents,name));
    const outside=path.join(output,'outside.docx');fs.copyFileSync(path.join(fixture,'actual.docx'),outside);
    win=new BrowserWindow({show:false,width:1000,height:800,webPreferences:{contextIsolation:true,nodeIntegration:false,preload:path.join(repo,'desktop/preload.bundle.cjs')}});
    const errors=[];win.webContents.on('console-message',(_e,_l,msg)=>errors.push(msg));await win.loadURL(process.env.R06_OFFICE_RENDERER);
    const js=source=>win.webContents.executeJavaScript(source,true);
    async function until(source,label){const stop=Date.now()+20000;while(Date.now()<stop){if(await js(`(()=>{try{return (${source})}catch{return false}})()`))return;await wait(25);}throw Error(label+': '+await js('document.body.innerText'));}
    await until('!!window.officeFixture','current production imports');
    await js(`window.officeFixture.open(${JSON.stringify(path.join(documents,'actual.docx'))},'actual.docx','docx')`);
    await until(`document.querySelector('.preview-docx')?.innerText.includes('实际中文')`,'real Word preview visible');
    const word=await js(`document.querySelector('.preview-docx').innerHTML`);assert.ok(word.includes('<h1>'));assert.ok(!word.includes('<script>'));
    await js(`window.officeFixture.open(${JSON.stringify(path.join(documents,'actual.xlsx'))},'actual.xlsx','xlsx')`);
    await until(`document.querySelector('.preview-csv')?.innerText.includes('第一张')`,'real Excel first sheet visible');
    const xlsx=await js(`document.querySelector('.preview-csv').innerHTML`);assert.ok(!xlsx.includes('不许返回第二张'));assert.ok(xlsx.includes('<table>'));assert.ok(!xlsx.includes('<script>'));
    await js(`window.officeFixture.open(${JSON.stringify(path.join(documents,'actual-dates.xlsx'))},'actual-dates.xlsx','xlsx')`);
    await until(`document.querySelector('.preview-csv')?.innerText.includes('2026-10-14T09:30:00.000Z')`,'actual Excel date renders its true UTC time');
    const dateVisible=await js(`document.querySelector('.preview-csv').innerText`);assert.ok(!dateVisible.includes('46309'));
    const failed=[];
    for(const name of ['broken.docx','broken.xlsx']){
      if(failed.length)await wait(5100);
      const ext=name.split('.').pop();const count=await js('window.officeFixture.store.getState().toasts.length');
      await js(`window.officeFixture.open(${JSON.stringify(path.join(documents,name))},${JSON.stringify(name)},${JSON.stringify(ext)})`);
      await until(`window.officeFixture.store.getState().toasts.length>${count} && !!document.querySelector('.toast-container .hana-toast.error') && document.querySelector('.toast-container').innerText.includes('文档预览失败')`,'actual current UI error visible');
      failed.push(name);
    }
    await wait(5100);
    const count=await js('window.officeFixture.store.getState().toasts.length');
    await js(`window.officeFixture.open(${JSON.stringify(outside)},'outside.docx','docx')`);
    await until(`window.officeFixture.store.getState().toasts.length>${count} && !!document.querySelector('.toast-container .hana-toast.error') && document.querySelector('.toast-container').innerText.includes('403')`,'outside scope visible refusal');
    assert.ok(requests.some(row=>row.url.includes(encodeURIComponent(outside))&&row.status===403));
    assert.ok(requests.filter(row=>row.url.includes('broken.')&&row.status===500).length===2);
    assert.equal(await js(`window.hana.readDocxHtml('relative')`),null);
    assert.ok(requests.filter(r=>r.url.includes('/fs/docx-html')).length>=3);assert.ok(requests.filter(r=>r.url.includes('/fs/xlsx-html')).length>=2);
    const sourcePreserved=['actual.docx','actual.xlsx','actual-dates.xlsx','broken.docx','broken.xlsx'].every(name=>digest(path.join(documents,name))===digest(path.join(fixture,name)));assert.ok(sourcePreserved);assert.equal(digest(binary),before);
    const result={status:'PASS_MAC_CURRENT_RUST_OFFICE_UI',binarySha256:before,word,xlsx,dateVisible,failed,sourcePreserved,requests,visible:await js('document.body.innerText'),errors};
    fs.writeFileSync(path.join(output,'result.json'),JSON.stringify(result,null,2));win.destroy();await stopRust();console.log('OFFICE_NATIVE_CURRENT_PASS');app.exit(0);
  }catch(error){console.error(error.stack||String(error));win?.destroy();service?.kill('SIGTERM');app.exit(1);}
});
