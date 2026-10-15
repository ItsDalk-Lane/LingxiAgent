// 真实 Chromium 按钮确认、拒绝和延后结果；业务网络直接到生产 Rust 服务。
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs'); const path = require('node:path'); const assert = require('node:assert/strict');
const home = process.env.R06_SESSION_HOME; const checks = []; const requests = []; const held = {}; let delayAnswers = false; let win;
app.setPath('userData', path.join(home, '.session-review-electron'));
app.commandLine.appendSwitch('disable-gpu');
const js = source => win.webContents.executeJavaScript(source, true);
async function until(source, label) { const deadline = Date.now() + 25000; while (Date.now() < deadline) {
  if (await js(`(()=>{try{return (${source})}catch{return false}})()`)) return;
  await new Promise(resolve => setTimeout(resolve, 20));
} throw Error(label + ': ' + await js('document.body.innerText')); }
function done(label) { checks.push(label); fs.writeFileSync(path.join(home, 'review-ui-' + label), 'actual UI action'); }
app.whenReady().then(async () => {
  const trace = text => fs.appendFileSync(path.join(home, 'review-ui-trace.log'), text + '\n'); trace('app-ready');
  win = new BrowserWindow({ show: false, width: 900, height: 750, webPreferences: { backgroundThrottling: false,
    contextIsolation: true, nodeIntegration: false, preload: path.join(__dirname, 'preload.cjs') } });
  win.webContents.on('console-message', (_event, _level, message) => trace('console ' + message));
  win.webContents.on('did-fail-load', (_event, code, description) => trace('load-failed ' + code + ' ' + description));
  trace('window-created');
  trace('loading ' + process.env.R06_SESSION_RENDERER);
  await win.loadURL(process.env.R06_SESSION_RENDERER); trace('loaded');
  win.webContents.debugger.attach('1.3'); await win.webContents.debugger.sendCommand('Network.enable');
  await win.webContents.debugger.sendCommand('Fetch.enable', { patterns: [{ urlPattern: '*/approvals/*', requestStage: 'Response' }] });
  win.webContents.debugger.on('message', (_event, method, params) => {
    if (method === 'Network.requestWillBeSent') requests.push({ method: params.request.method, url: params.request.url });
    if (method === 'Fetch.requestPaused') {
      const session = params.request.url.includes('/sessions/tool-a/') ? 'a' : 'b';
      if (delayAnswers && params.request.method === 'POST') held[session] = params.requestId;
      else void win.webContents.debugger.sendCommand('Fetch.continueRequest', { requestId: params.requestId });
    }
  });

  await until(`!!window.sessionReviewFixture`, 'production review surface'); done('ready');
  await until(`document.body.innerText.includes('tracked.txt') && [...document.querySelectorAll('button')].some(b=>b.innerText==='拒绝')`, 'real scoped approval visible');
  while (!fs.existsSync(path.join(home, 'review-ui-reject-ready'))) await new Promise(resolve => setTimeout(resolve, 20));
  await js(`[...document.querySelectorAll('button')].find(b=>b.innerText==='拒绝').click()`); done('rejected');
  await until(`![...document.querySelectorAll('button')].some(b=>b.innerText==='确认执行')`, 'rejected request disappears');
  await until(`[...document.querySelectorAll('button')].some(b=>b.innerText==='确认执行')`, 'second fresh approval visible');
  while (!fs.existsSync(path.join(home, 'review-ui-confirm-ready'))) await new Promise(resolve => setTimeout(resolve, 20));
  await js(`[...document.querySelectorAll('button')].find(b=>b.innerText==='确认执行').click()`); done('confirmed');
  await until(`document.body.innerText.includes('等待本轮结束后回退')`, 'scheduled status must not claim completion'); done('scheduled');
  await until(`document.body.innerText.includes('已回退到检查点')`, 'durable completion visible'); done('completed');
  await until(`[...document.querySelectorAll('button')].some(b=>b.innerText==='确认执行')`, 'third fresh actual approval');
  const other = { baseUrl: process.env.R06_SESSION_OTHER_URL, token: process.env.R06_SESSION_OTHER_TOKEN };
  await js(`(()=>{const store=window.sessionReviewFixture.store;const s=store.getState();window.reviewOriginalConnection=s.activeServerConnection;const button=[...document.querySelectorAll('button')].find(b=>b.innerText==='确认执行');const c={...s.activeServerConnection,connectionId:'other-service',baseUrl:${JSON.stringify(other.baseUrl)},wsUrl:${JSON.stringify(other.baseUrl.replace('http:','ws:'))},token:${JSON.stringify(other.token)}};store.setState({activeServerConnection:c,activeServerConnectionId:c.connectionId,serverConnections:{...s.serverConnections,[c.connectionId]:c}});store.setState({currentSessionId:'tool-a',currentSessionPath:'native:tool-a'});button.click();})()`);
  await until(`!document.body.innerText.includes('丢弃其后分支')`, 'old approval synchronously invalid after owner change');
  await js(`(()=>{const store=window.sessionReviewFixture.store;const c=window.reviewOriginalConnection;store.setState({activeServerConnection:c,activeServerConnectionId:c.connectionId});store.setState({currentSessionId:'tool-a',currentSessionPath:'native:tool-a'});})()`);
  await until(`[...document.querySelectorAll('button')].some(b=>b.innerText==='确认执行')`, 'same actual pending approval freshly loaded on original owner');
  await js(`(()=>{const store=window.sessionReviewFixture.store;const s=store.getState();const c=s.activeServerConnection;const button=[...document.querySelectorAll('button')].find(b=>b.innerText==='确认执行');const changed={...c,baseUrl:${JSON.stringify(other.baseUrl)},wsUrl:${JSON.stringify(other.baseUrl.replace('http:','ws:'))},token:${JSON.stringify(other.token)}};store.setState({serverConnections:{...s.serverConnections,[c.connectionId]:changed}});store.setState({serverConnections:{...s.serverConnections,[c.connectionId]:c}});button.click();})()`);
  await new Promise(resolve=>setTimeout(resolve,200));
  const otherServicePosts = requests.filter(request=>request.method==='POST'&&request.url.startsWith(other.baseUrl+'/'));
  assert.deepEqual(otherServicePosts, []); done('isolated');
  // 批准已由真实服务处理，仅暂停浏览器接收响应，复现同连接旧会话迟到回包。
  while (!fs.existsSync(path.join(home, 'review-ui-delay-a-ready'))) await new Promise(resolve => setTimeout(resolve,20));
  const aid = JSON.parse(fs.readFileSync(path.join(home, 'review-ui-delay-a-ready'))).approvalId;
  await js(`window.sessionReviewFixture.store.setState({currentSessionId:'tool-a',currentSessionPath:'native:tool-a'})`);
  await until(`!!document.querySelector('[data-approval-id="${aid}"] button')`, 'fresh A approval for delayed actual response');
  delayAnswers = true;
  await js(`document.querySelector('[data-approval-id="${aid}"] button').click()`);
  while (!held.a) await new Promise(resolve => setTimeout(resolve,20)); done('delay-a-held');
  while (!fs.existsSync(path.join(home, 'review-ui-delay-b-ready'))) await new Promise(resolve => setTimeout(resolve,20));
  const bid = JSON.parse(fs.readFileSync(path.join(home, 'review-ui-delay-b-ready'))).approvalId;
  await js(`(()=>{const button=document.querySelector('[data-approval-id="${aid}"] button');window.sessionReviewFixture.store.setState({currentSessionId:'tool-b',currentSessionPath:'native:tool-b'});if(button)button.click();})()`);
  await until(`!!document.querySelector('[data-approval-id="${bid}"] button')`, 'fresh B approval');
  await js(`document.querySelector('[data-approval-id="${bid}"] button').click()`);
  while (!held.b) await new Promise(resolve => setTimeout(resolve,20));
  assert.equal(await js(`document.querySelector('[data-testid="rust-session-review"]').getAttribute('aria-busy')`), 'true');
  await win.webContents.debugger.sendCommand('Fetch.continueRequest', {requestId:held.a});
  await new Promise(resolve=>setTimeout(resolve,150));
  assert.equal(await js(`document.querySelector('[data-testid="rust-session-review"]').getAttribute('aria-busy')`), 'true', 'late A finally cannot clear B busy');
  assert.equal(await js(`(()=>{const b=document.querySelector('[data-approval-id="${bid}"] button');return !b || b.disabled})()`), true);
  assert.equal(await js(`window.sessionReviewFixture.store.getState().currentSessionId`), 'tool-b');
  await win.webContents.debugger.sendCommand('Fetch.continueRequest', {requestId:held.b}); delayAnswers=false;
  await until(`!document.querySelector('[data-approval-id="${bid}"]') && document.querySelector('[data-testid="rust-session-review"]').getAttribute('aria-busy')==='false'`, 'B own response settles its approval'); done('session-delay-isolated');
  const posts = requests.filter(request => request.method === 'POST' && request.url.includes('/approvals/'));
  assert.equal(posts.length, 4); assert.ok(posts.every(request => request.url.startsWith(process.env.R06_SESSION_URL + '/')));
  fs.writeFileSync(path.join(home, 'review-ui-result.json'), JSON.stringify({ status: 'PASS', checks, posts, otherServicePosts, requests, visible: await js('document.body.innerText') }, null, 2));
  win.destroy(); app.quit();
}).catch(async error => { fs.writeFileSync(path.join(home, 'review-ui-failure.json'), JSON.stringify({ error: String(error.stack || error), checks, requests, visible: win && !win.isDestroyed() ? await js('document.body.innerText') : null }, null, 2)); console.error(error); app.exit(1); });
