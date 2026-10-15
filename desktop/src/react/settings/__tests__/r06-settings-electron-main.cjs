// 真正桌面窗口加载当前生产设置入口；逐动作检查 DOM、真实 HTTP 与合成磁盘。
const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const home = process.env.R06_SETTINGS_HOME;
const url = process.env.R06_SETTINGS_URL;
const token = process.env.R06_SETTINGS_TOKEN;
const scenario = process.env.R06_SETTINGS_CASE || 'smoke';
if (!home || !url || !token) throw new Error('synthetic native service fixture is required');
app.setPath('userData', path.join(home, '.electron-fixture-profile'));
app.commandLine.appendSwitch('disable-gpu');
const requests = [];
const reveals = [];
const checks = [];
let window;
let chooser;
let cardNetwork;
let dreamRuns;
function check(name, value) { assert.ok(value, name); checks.push(name); }
async function api(method, route, body) {
  const response = await fetch(url + route, { method, headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  const data = await response.json();
  assert.ok(response.ok, `${method} ${route}: ${response.status} ${JSON.stringify(data)}`);
  return data;
}
async function js(source) { return window.webContents.executeJavaScript(source, true); }
async function until(source, message, ms = 15_000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) { if (await js(`(() => { try { return (${source}); } catch { return false; } })()`)) return; await new Promise(resolve => setTimeout(resolve, 25)); }
  throw new Error(`${message}\n${await js('document.body.innerText.slice(-10000)')}`);
}
async function click(key) {
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t(${JSON.stringify(key)}) && !node.disabled)`, 'actual enabled button ' + key);
  await js(`window.__r06.click(${JSON.stringify(key)})`);
}
async function input(selector, value) { await js(`window.__r06.input(${JSON.stringify(selector)}, ${JSON.stringify(value)})`); }
async function toggle(key) { await js(`window.__r06.toggle(${JSON.stringify(key)})`); }
async function selectAgent(id) {
  await input('select[aria-label="助手"]', id);
  await until(`window.__r06.store.getState().getSettingsAgentId() === ${JSON.stringify(id)} && window.__r06.store.getState().settingsConfigStatus === 'ready'`, 'browse agent actual configuration');
}
async function fileInput(selector, file) {
  const { root } = await window.webContents.debugger.sendCommand('DOM.getDocument');
  const { nodeId } = await window.webContents.debugger.sendCommand('DOM.querySelector', { nodeId: root.nodeId, selector });
  assert.ok(nodeId, `actual file input ${selector}`);
  await window.webContents.debugger.sendCommand('DOM.setFileInputFiles', { nodeId, files: file ? [file] : [] });
}
async function avatarFile(file) {
  const selected = await js('window.__r06.store.getState().getSettingsAgentId()');
  const event = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => { chooser = null; reject(new Error('actual avatar file chooser did not open')); }, 5000);
    chooser = params => { clearTimeout(timeout); resolve(params); };
  });
  await js(`document.querySelector('[data-agent-id="${selected}"]').click()`);
  const params = await event;
  await window.webContents.debugger.sendCommand('DOM.setFileInputFiles', { backendNodeId: params.backendNodeId, files: file ? [file] : [] });
}
ipcMain.handle('fixture:connection', () => ({ port: Number(new URL(cardNetwork?.base || url).port), token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }));
ipcMain.handle('fixture:show-in-finder', (_event, filePath) => {
  assert.ok(fs.statSync(filePath).isFile(), 'reveal request points at actually exported file');
  assert.ok(path.resolve(filePath).startsWith(path.resolve(home, '..') + path.sep), 'export stays in synthetic workspace');
  reveals.push(filePath);
});
app.whenReady().then(async () => {
  if (['card-owner','failures','initialization','boundaries'].includes(scenario)) cardNetwork = await prepareCardNetwork();
  window = new BrowserWindow({ show: false, width: 1180, height: 950,
    webPreferences: { backgroundThrottling: false, preload: path.join(__dirname, 'r06-settings-electron-preload.cjs'), contextIsolation: true, nodeIntegration: false } });
  window.webContents.session.webRequest.onCompleted(details => {
    const parsed = new URL(details.url);
    const otherFile = path.join(home, 'other-connection.json');
    const otherOrigin = fs.existsSync(otherFile) ? new URL(JSON.parse(fs.readFileSync(otherFile, 'utf8')).url).origin : '';
    if ([new URL(url).origin, cardNetwork?.base, otherOrigin].includes(parsed.origin)) requests.push({ method: details.method, path: parsed.pathname, status: details.statusCode, origin: parsed.origin });
  });
  console.log('ELECTRON-FIXTURE: loading actual settings entry');
  await window.loadURL(process.env.R06_SETTINGS_RENDERER_URL);
  console.log('ELECTRON-FIXTURE: actual entry loaded');
  window.webContents.debugger.attach('1.3');
  window.webContents.debugger.on('message', (_event, method, params) => {
    if (method === 'Page.fileChooserOpened' && chooser) { const received = chooser; chooser = null; received(params); }
  });
  await window.webContents.debugger.sendCommand('Page.enable');
  await window.webContents.debugger.sendCommand('Page.setInterceptFileChooserDialog', { enabled: true });
  await js(`(async () => {
    const module = await import('/react/settings/store.ts');
    window.__r06 = {
      store: module.useSettingsStore,
      click(key) {
        const label = window.t(key);
        const button = Array.from(document.querySelectorAll('button')).findLast(node => node.textContent.trim() === label && !node.disabled);
        if (!button) throw new Error('actual enabled button missing: ' + key + ' / ' + label);
        button.click();
      },
      input(selector, value) {
        const node = document.querySelector(selector);
        if (!node || node.disabled) throw new Error('actual enabled input missing: ' + selector);
        const prototype = node instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : node instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
        Object.getOwnPropertyDescriptor(prototype, 'value').set.call(node, value);
        node.dispatchEvent(new Event(node instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
      },
      toggle(key) {
        const label = window.t(key);
        let node = Array.from(document.querySelectorAll('div,span,label,h2,h3')).find(node => node.childElementCount === 0 && node.textContent.trim() === label);
        while (node && !node.querySelector('button[role="switch"]')) node = node.parentElement;
        const control = node?.querySelector('button[role="switch"]');
        if (!control || control.disabled) throw new Error('actual enabled toggle missing: ' + key + ' / ' + label);
        control.click();
      },
    };
  })()`);
  await until('window.__r06.store.getState().ready', 'current settings initialization');
  await click('settings.tabs.agent');
  if (scenario === 'read-denied') {
    await until(`!!document.querySelector('[role=alert]')`, 'real denied initialization visible error');
    check('denied-ready-unmasked', await js('window.__r06.store.getState().ready'));
    check('denied-explicit-error-state', await js(`window.__r06.store.getState().settingsConfigStatus === 'error' || !!window.__r06.store.getState().agentsError`));
    check('denied-actual-403', requests.some(request => request.status === 403));
    check('denied-zero-mutations', !requests.some(request => ['PUT','POST','DELETE'].includes(request.method)));
    console.log('R06_SETTINGS_ELECTRON_RESULT:' + JSON.stringify({ scenario, checks, requests, reveals, dreamRuns }));
    window.destroy(); app.exit(0); return;
  }
  await until('window.__r06.store.getState().settingsConfigStatus === "ready" && !!document.querySelector("select[aria-label=助手]") && !document.querySelector("select[aria-label=助手]").disabled', 'native agent page readiness');
  check('production-settings-entry', await js('!!document.querySelector("#settingsPanel") && !!window.hana && window.platform === window.hana'));
  check('actual-agents-list', requests.some(request => request.path === '/lingxi/v1/agents' && request.status === 200));
  check('actual-settings-snapshot', requests.some(request => request.path === '/lingxi/v1/settings/snapshot' && request.status === 200));
  check('actual-model-catalog', requests.some(request => request.path === '/lingxi/v1/models' && request.status === 200));
  check('actual-current-model-binding', await js('window.__r06.store.getState().globalModelsConfig.models.memory.id === "memory-model"'));
  await until(`document.body.innerText.includes('daily_pipeline_absent_in_candidate')`, 'health response truthful loaded');
  const health = await api('GET', '/lingxi/v1/memories/health?agentId=' + (scenario === 'receipt-owner' ? 'agentB' : 'agentA'));
  const ticker = health.steps.find(step => step.step === 'memory_ticker');
  check('explicit-maintenance-state', ticker.scope === 'automatic_dream' && ticker.ticker.available === true && await js('document.body.innerText.includes("daily_pipeline_absent_in_candidate")'));
  if (scenario === 'workflow') await workflow();
  else if (scenario === 'references-pending') await referencePending();
  else if (scenario === 'receipt-owner') await receiptOwner();
  else if (scenario === 'card-owner') await cardOwner();
  else if (scenario === 'initialization') await initializationBoundaries();
  else if (scenario === 'boundaries') await remainingBoundaries();
  else if (scenario === 'failures') await failures();
  else if (scenario !== 'smoke') throw new Error('fixture workflow is not implemented: ' + scenario);
  if (['failures','initialization','boundaries'].includes(scenario)) { cardNetwork.release(); cardNetwork.server.closeAllConnections(); await new Promise(resolve => cardNetwork.server.close(resolve)); }
  console.log('R06_SETTINGS_ELECTRON_RESULT:' + JSON.stringify({ scenario, checks, requests, reveals, dreamRuns }));
  window.destroy(); app.exit(0);
}).catch(error => { console.error(error.stack || String(error)); if (window && !window.isDestroyed()) window.destroy(); app.exit(1); });

async function workflow() {
  const savedB = fs.readFileSync(path.join(home, 'agents/agentB/config.yaml'));
  const config = () => api('GET', '/lingxi/v1/agents/agentA/config');
  const waitConfig = async (expression) => until(`window.__r06.store.getState().settingsConfigStatus === 'ready' && (${expression})`, 'actual config readback');
  const titleClick = async key => js(`document.querySelector('button[title="' + window.t(${JSON.stringify(key)}) + '"]').click()`);
  const closeViewer = async () => {
    await js(`(() => { const titles = [window.t('settings.memory.compiled'), window.t('settings.memory.actions.viewTitle')]; const headers = Array.from(document.querySelectorAll('h3')).filter(node => titles.includes(node.textContent.trim()) && Array.from(node.parentElement.querySelectorAll('button')).some(button => button.textContent.trim() === '✕')); Array.from(headers.at(-1).parentElement.querySelectorAll('button')).find(button => button.textContent.trim() === '✕').click(); })()`);
    await until(`!Array.from(document.querySelectorAll('h3')).some(node => [window.t('settings.memory.compiled'), window.t('settings.memory.actions.viewTitle')].includes(node.textContent.trim()) && Array.from(node.parentElement.querySelectorAll('button')).some(button => button.textContent.trim() === '✕'))`, 'viewer closed and unmounted');
  };
  check('00-container-real-load', requests.some(request => request.path === '/lingxi/v1/providers/summary' && request.status === 200));
  await selectAgent('agentB');
  check('01-browse-six-real-reads', await js(`window.__r06.store.getState().settingsConfig._identity === 'agentB已保存身份'`));
  await selectAgent('agentA');
  await input('input[placeholder="' + await js("window.t('settings.agent.agentNameHint')") + '"]', 'UI真实改名');
  await js(`document.querySelector('input[placeholder="' + window.t('settings.agent.agentNameHint') + '"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await waitConfig("window.__r06.store.getState().settingsConfig.agent.name === 'UI真实改名'");
  check('02-name-persist-and-readback', (await config()).agent.name === 'UI真实改名');
  await click('settings.agent.setPrimary');
  await until(`window.__r06.store.getState().agents.some(agent => agent.id === 'agentA' && agent.isPrimary)`, 'actual primary agent readback');
  check('02-primary-actual-preferences', JSON.parse(fs.readFileSync(path.join(home, 'user/preferences.json'), 'utf8')).primary_agent_id === 'agentA');
  await js(`document.querySelector('button[aria-haspopup=listbox]').click()`);
  await until(`Array.from(document.querySelectorAll('[role=option]')).some(node => node.textContent.trim() === 'chat-model')`, 'actual model options');
  await js(`Array.from(document.querySelectorAll('[role=option]')).find(node => node.textContent.trim() === 'chat-model').click()`);
  await waitConfig(`window.__r06.store.getState().settingsConfig.models.chat.id === 'chat-model'`);
  check('02-chat-model-real-save', (await config()).models.chat.provider === 'persona-stub');
  await js(`Array.from(document.querySelectorAll('.yuan-chip')).find(node => node.textContent.includes('ming')).click()`);
  await waitConfig("window.__r06.store.getState().settingsConfig.agent.yuan === 'ming'");
  await js(`window.__r06.input('textarea', '页面真实身份')`);
  const behaviorSelector = await js(`Array.from(document.querySelectorAll('textarea')).filter(node => node.value === 'agentA已保存行为').map(node => { node.id='r06-real-behavior'; return '#'+node.id; })[0]`);
  await input(behaviorSelector, '页面真实行为');
  await js(`Array.from(document.querySelectorAll('button')).filter(node => node.textContent.trim() === window.t('settings.save')).at(-1).click()`);
  await waitConfig("window.__r06.store.getState().settingsConfig._identity === '页面真实身份'");
  check('03-yuan-persona-independent-writes', fs.readFileSync(path.join(home, 'agents/agentA/identity.md'), 'utf8') === '页面真实身份' && fs.readFileSync(path.join(home, 'agents/agentA/AGENTS.md'), 'utf8') === '页面真实行为');
  await toggle('settings.memory.sectionTitle');
  await waitConfig("window.__r06.store.getState().settingsConfig.memory.enabled === false");
  check('04-memory-real-toggle', (await config()).memory.enabled === false);
  await toggle('settings.memory.sectionTitle');
  await waitConfig("window.__r06.store.getState().settingsConfig.memory.enabled === true");
  await toggle('settings.experience.toggleLabel');
  await waitConfig("window.__r06.store.getState().settingsConfig.experience.enabled === true");
  await until("document.body.innerText.includes('真实经验条目')", 'conditional experience read');
  await titleClick('settings.experience.edit');
  await js(`const node = Array.from(document.querySelectorAll('textarea')).find(node => node.value.includes('真实经验条目')); node.id='r06-real-experience';`);
  await input('#r06-real-experience', '1. 实际编辑经验');
  await js(`document.querySelector('#r06-real-experience').dispatchEvent(new FocusEvent('focusout', { bubbles: true }))`);
  await until("!document.querySelector('#r06-real-experience')", 'experience editor saved');
  for (let poll = 0; poll < 200 && !requests.some(request => request.path === '/lingxi/v1/agents/agentA/experience' && request.method === 'PUT' && request.status === 200); poll++) await new Promise(resolve => setTimeout(resolve, 10));
  check('05-experience-real-request', requests.some(request => request.path === '/lingxi/v1/agents/agentA/experience' && request.method === 'PUT' && request.status === 200));
  check('05-experience-real-edit', (await api('GET', '/lingxi/v1/agents/agentA/experience')).content.includes('实际编辑经验'));
  await click('settings.agent.exportAgent');
  await until("!!document.querySelector('[role=dialog] input[type=checkbox]')", 'real export preview');
  await until(`Array.from(document.querySelectorAll('[role=dialog] img')).every(img => img.complete && img.naturalWidth > 0)`, 'real asset image decode');
  await click('settings.characterCard.cancel');
  const exportsBefore = reveals.length;
  await click('settings.agent.exportAgent');
  await until("!!document.querySelector('[role=dialog] input[type=checkbox]')", 'export preview reopened');
  await click('settings.characterCard.confirm');
  await until("!document.querySelector('[role=dialog] input[type=checkbox]')", 'real export finished');
  await new Promise(resolve => setTimeout(resolve, 100));
  check('06-character-export-real-file-and-reveal', reveals.length === exportsBefore + 1 && fs.statSync(reveals.at(-1)).size > 0);
  const tool = await js(`document.querySelector('[data-tool-name="automation"] button[role=switch]')?.getAttribute('aria-checked')`);
  await js(`document.querySelector('[data-tool-name="automation"] button[role=switch]').click()`);
  await waitConfig("window.__r06.store.getState().settingsConfig.tools.disabled.includes('automation')");
  check('07-optional-tools-real-save', (await config()).tools.disabled.includes('automation') && tool === 'true');
  for (const [action, name] of [['08', 'beautify'], ['09', 'office']]) {
    await js(`document.querySelector('[data-tool-name="${name}"] button[role=switch]').click()`);
    await waitConfig(`window.__r06.store.getState().settingsConfig.tools.disabled.includes('${name}')`);
    check(action + '-' + name + '-actual-registered-toggle', (await config()).tools.disabled.includes(name));
  }
  check('05-experience-cache-survives-config-refresh', await js(`document.body.innerText.includes('实际编辑经验') && !document.body.innerText.includes('真实经验条目')`));
  await titleClick('settings.experience.deleteCategory');
  await until(`document.body.innerText.includes(window.t('settings.experience.empty'))`, 'experience category actual optimistic delete');
  for (let poll = 0; poll < 200 && (await api('GET', '/lingxi/v1/agents/agentA/experience')).content !== ''; poll++) await new Promise(resolve => setTimeout(resolve, 10));
  check('05-experience-category-deleted-real', (await api('GET', '/lingxi/v1/agents/agentA/experience')).content === '');
  const avatarPath = path.join(home, '..', 'desktop/src/assets/Lingxi.png');
  await avatarFile(avatarPath);
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.crop.cancel'))`, 'real avatar crop image');
  await click('settings.crop.cancel');
  await until(`!Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.crop.confirm'))`, 'cancelled crop actually unmounted');
  check('10-avatar-cancel-no-post', !requests.some(request => request.method === 'POST' && request.path.endsWith('/avatar')));
  await avatarFile(avatarPath);
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.crop.confirm'))`, 'real avatar crop reopened');
  await click('settings.crop.confirm');
  await until(`window.__r06.store.getState().agents.some(agent => agent.id === 'agentA' && agent.hasAvatar === true)`, 'actual uploaded avatar readback');
  check('10-avatar-canvas-upload-real', fs.statSync(path.join(home, 'agents/agentA/avatars/agent.png')).size > 0);
  await click('settings.memory.compiledView');
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.memory.editEntry') && !node.disabled)`, 'compiled sections actual load');
  await click('settings.memory.editEntry');
  await input('#compiled-memory-today-editor', '页面实际今日记忆');
  await input('#compiled-memory-facts-editor', '- 页面实际事实记忆');
  await input('#compiled-memory-longterm-editor', '页面实际长期记忆');
  const days = await api('GET', '/lingxi/v1/memories/compiled/week/days?agentId=agentA');
  check('11-real-day-loaded', days.days.length === 1);
  await input('textarea[aria-label="' + days.days[0].date + '"]', '页面实际每日内容');
  await click('settings.memory.editSave');
  await until("!document.querySelector('#compiled-memory-facts-editor')", 'compiled actual writes completed');
  check('11-compiled-three-real-writes', (await api('GET', '/lingxi/v1/memories/compiled?agentId=agentA')).sections.today === '页面实际今日记忆');
  check('11-daily-edit-real-file', fs.readFileSync(path.join(home, 'agents/agentA/memory/daily', days.days[0].date + '.md'), 'utf8').trim() === '页面实际每日内容');
  await closeViewer();
  await click('settings.memory.actions.view');
  await until("document.body.innerText.includes('Electron实际结构事实')", 'real whole memory viewer');
  await closeViewer();
  const deletes = requests.filter(request => request.method === 'DELETE').length;
  await click('settings.memory.actions.clear'); await click('settings.memory.actions.cancel');
  check('12-whole-memory-cancel-no-delete', requests.filter(request => request.method === 'DELETE').length === deletes);
  await input('select:has(option[value="embedding-stub/embedding-model"])', 'embedding-stub/embedding-model');
  await until(`document.querySelector('select:has(option[value="embedding-stub/embedding-model"])')?.disabled === false`, 'embedding persist');
  check('13-embedding-persist', (await config()).memory.embedding_model.id === 'embedding-model');
  await input('select:has(option[value="embedding-stub/embedding-model"])', '');
  await until(`document.querySelector('select:has(option[value="embedding-stub/embedding-model"])')?.disabled === false`, 'embedding explicit clear');
  check('13-embedding-clear', !((await config()).memory.embedding_model?.provider) && !fs.readFileSync(path.join(home, 'agents/agentA/config.yaml'), 'utf8').includes('embedding_model'));
  await input('input[placeholder="' + await js("window.t('settings.memory.tenetsAddPlaceholder')") + '"]', '页面真实新增原则');
  await js(`document.querySelector('input[placeholder="' + window.t('settings.memory.tenetsAddPlaceholder') + '"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await until(`document.body.innerText.includes('页面真实新增原则') && !document.querySelector('input[maxlength="300"]').value`, 'tenet real POST and GET');
  await js(`Array.from(document.querySelectorAll('span')).find(node => node.textContent === '待批准真实原则').parentElement.querySelector('button').click()`);
  await until(`Array.from(document.querySelectorAll('span')).some(node => node.textContent === '待批准真实原则' && node.parentElement.innerText.includes(window.t('settings.memory.tenetsDelete')))`, 'tenet decide readback');
  check('14-tenets-add-and-approve-real', (await api('GET', '/lingxi/v1/agents/agentA/tenets')).tenets.some(tenet => tenet.content === '待批准真实原则' && tenet.status === 'active'));
  await js(`Array.from(document.querySelectorAll('span')).find(node => node.textContent === '待拒绝真实原则').parentElement.querySelectorAll('button')[1].click()`);
  await until(`!document.body.innerText.includes('待拒绝真实原则')`, 'tenet actual rejection readback');
  check('14-tenet-reject-real', (await api('GET', '/lingxi/v1/agents/agentA/tenets')).tenets.some(tenet => tenet.content === '待拒绝真实原则' && tenet.status === 'rejected'));
  await js(`Array.from(document.querySelectorAll('span')).find(node => node.textContent === '页面真实新增原则').parentElement.querySelector('button').click()`);
  await until(`!document.body.innerText.includes('页面真实新增原则')`, 'tenet actual delete readback');
  check('14-tenet-delete-real', !(await api('GET', '/lingxi/v1/agents/agentA/tenets')).tenets.some(tenet => tenet.content === '页面真实新增原则'));
  await toggle('settings.memory.dream.autoLabel');
  await until(`window.__r06.store.getState().settingsConfig.memory.dream.auto_enabled === true`, 'dream auto actual PUT/GET');
  let automatic;
  for (let poll=0; poll<400; poll++) {
    automatic = await api('GET', '/lingxi/v1/memories/dream/status?agentId=agentA');
    if (automatic.status !== 'running' && automatic.lastRun?.trigger === 'automatic') break;
    await new Promise(resolve => setTimeout(resolve,25));
  }
  check('15-auto-enabled-actual-distinct-completed-run', automatic.lastRun?.trigger === 'automatic' && automatic.lastRun.status === 'succeeded');
  const automaticId=automatic.lastRun.runId;
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.memory.dream.run') && !node.disabled)`, 'automatic completed before explicit manual click');
  const manualPosts=requests.filter(request=>request.path==='/lingxi/v1/memories/dream/runs'&&request.method==='POST').length;
  await click('settings.memory.dream.run');
  let dream;
  for (let poll=0; poll<400; poll++) {
    dream=await api('GET','/lingxi/v1/memories/dream/status?agentId=agentA');
    if (dream.status!=='running' && dream.lastRun?.trigger==='manual' && dream.lastRun?.runId!==automaticId) break;
    await new Promise(resolve => setTimeout(resolve,25));
  }
  check('15-paid-dream-real-success', dream.status==='succeeded' && dream.lastRun.status==='succeeded' && dream.lastRun.trigger==='manual' && dream.lastRun.runId!==automaticId && requests.filter(request=>request.path==='/lingxi/v1/memories/dream/runs'&&request.method==='POST'&&request.status===202).length===manualPosts+1);
  dreamRuns={automatic:automatic.lastRun,manual:dream.lastRun};
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.memory.dream.run') && !node.disabled)`, 'actual Dream report refreshed');
  await click('settings.memory.dream.restore');
  await until("!!document.querySelector('#dream-revisions-title')", 'revision list actual load');
  await until(`document.querySelector('[aria-label="' + window.t('settings.memory.dream.revisions.listLabel') + '"] button') !== null`, 'actual revision available');
  await js(`document.querySelector('[aria-label="' + window.t('settings.memory.dream.revisions.listLabel') + '"] button').click()`);
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.memory.dream.revisions.restoreThis') && !node.disabled)`, 'actual revision detail diff');
  check('16-revision-real-list-and-diff', requests.some(request => request.path.startsWith('/lingxi/v1/memories/dream/revisions/') && request.status === 200));
  await click('settings.memory.dream.revisions.restoreThis');
  await click('settings.memory.dream.revisions.confirmRestore');
  await until(`document.body.innerText.includes(window.t('settings.memory.dream.revisions.restored'))`, 'actual revision restore');
  await click('settings.memory.dream.revisions.close');
  check('17-health-real-degraded', requests.some(request => request.path === '/lingxi/v1/memories/health' && request.status === 200) && await js(`document.body.innerText.includes('daily_pipeline_absent_in_candidate')`));
  // 保留生产每十秒限流；批量自动操作跨窗口推进，不能提高或绕过限额。
  await new Promise(resolve => setTimeout(resolve, 10_100));
  // 手工创建与人物卡导入都从现役页面控件触发，创建和实际切换分别核对。
  await js(`document.querySelector('[data-add-btn="1"]').click()`);
  await until(`!!document.querySelector('input[placeholder="' + window.t('settings.agent.namePlaceholder') + '"]')`, 'actual create dialog');
  const createsBefore = requests.filter(request => request.path === '/lingxi/v1/agents' && request.method === 'POST').length;
  await click('settings.agent.confirm');
  await until(`document.body.innerText.includes(window.t('settings.agent.nameRequired'))`, 'missing create name explicit error');
  check('01-create-empty-no-post', createsBefore === requests.filter(request => request.path === '/lingxi/v1/agents' && request.method === 'POST').length);
  await input('input[placeholder="' + await js("window.t('settings.agent.namePlaceholder')") + '"]', '窗口新增助手');
  await click('settings.agent.confirm');
  await until(`window.__r06.store.getState().agents.some(agent => agent.name === '窗口新增助手') && window.__r06.store.getState().settingsConfigStatus === 'ready'`, 'actual manual create and subsequent config');
  const created = (await api('GET', '/lingxi/v1/agents')).agents.find(agent => agent.name === '窗口新增助手');
  check('01-manual-create-real-switch', JSON.parse(fs.readFileSync(path.join(home, 'user/preferences.json'), 'utf8')).current_agent_id === created.id);
  await selectAgent('agentA');
  await click('settings.agent.exportAgent');
  await until(`!!document.querySelector('[role=dialog] input[type=checkbox]:not(:disabled)')`, 'export available memory');
  await js(`document.querySelector('[role=dialog] input[type=checkbox]').click()`);
  await click('settings.characterCard.confirm');
  await until(`!document.querySelector('[role=dialog] input[type=checkbox]')`, 'export includes selected actual memory');
  await new Promise(resolve => setTimeout(resolve, 100));
  const exported = reveals.at(-1);
  await js(`document.querySelector('[data-add-btn="1"]').click()`);
  await until(`!!document.querySelector('input[type=file][accept*=hana-package]')`, 'actual import file control');
  await fileInput('input[type=file][accept*=hana-package]', exported);
  await until(`!!document.querySelector('[role=dialog] input[type=checkbox]:not(:disabled)')`, 'actual multipart ZIP plan');
  const importsBefore = requests.filter(request => request.path === '/lingxi/v1/character-cards/import' && request.method === 'POST').length;
  check('01-import-plan-does-not-write-agent', importsBefore === 0);
  await click('settings.characterCard.confirm');
  await until(`window.__r06.store.getState().currentAgentId?.startsWith('card-') && window.__r06.store.getState().settingsConfigStatus === 'ready'`, 'actual imported agent switch');
  const importedId = await js(`window.__r06.store.getState().currentAgentId`);
  const importedMemories = await api('GET', '/lingxi/v1/memories?agentId=' + importedId);
  check('01-import-unchecked-excludes-memory', importedMemories.memories.length === 0 && fs.readFileSync(path.join(home, 'agents', importedId, 'identity.md'), 'utf8') === '页面真实身份');
  await selectAgent('agentA');
  const beforeOrder = (await api('GET', '/lingxi/v1/agents')).agents.map(agent => agent.id);
  const cardId = beforeOrder[0];
  // Electron 的真实鼠标输入需要原生窗口获得焦点；此短窗口只使用本轮合成资料。
  window.show(); app.focus({ steal: true }); window.focus();
  await new Promise(resolve => setTimeout(resolve, 300));
  await js(`document.querySelector('[data-agent-id="${cardId}"]').scrollIntoView({ block: 'center', inline: 'center' })`);
  let rect = await js(`(() => { const node = document.querySelector('[data-agent-id="${cardId}"]'); const box=node.getBoundingClientRect(); return {x: box.x, y:box.y,width:box.width,height:box.height}; })()`);
  window.webContents.sendInputEvent({ type: 'mouseMove', x: Math.round(rect.x + 30), y: Math.round(rect.y + 30) });
  await new Promise(resolve => setTimeout(resolve, 400));
  const point = await js(`(() => { const id=${JSON.stringify(cardId)}; const box=document.querySelector('[data-agent-id="'+id+'"]').getBoundingClientRect(); for(let y=Math.max(0,box.top+5); y<Math.min(innerHeight,box.bottom-5); y+=5) for(let x=Math.max(0,box.left+5); x<Math.min(innerWidth,box.right-5); x+=5) if(document.elementFromPoint(x,y)?.closest('[data-agent-id]')?.dataset.agentId === id) return {x,y}; return null; })()`);
  check('18-order-visible-real-drag-target', !!point);
  console.log('DRAG-FIXTURE:' + JSON.stringify(await js(`(() => { const node=document.querySelector('[data-agent-id="${cardId}"]'); return { point:${JSON.stringify(point)}, hover: node.parentElement.matches(':hover'), indices: Array.from(node.parentElement.children).map(item => ({ id:item.dataset.agentId, x:item.style.getPropertyValue('--tx-spread') })) }; })()`)));
  window.webContents.sendInputEvent({ type: 'mouseDown', x: Math.round(point.x), y: Math.round(point.y), button: 'left', clickCount: 1 });
  window.webContents.sendInputEvent({ type: 'mouseMove', x: Math.round(point.x + 160), y: Math.round(point.y), modifiers: ['leftButtonDown'] });
  await new Promise(resolve => setTimeout(resolve, 100));
  window.webContents.sendInputEvent({ type: 'mouseUp', x: Math.round(point.x + 160), y: Math.round(point.y), button: 'left', clickCount: 1 });
  console.log('DRAG-FIXTURE-AFTER:' + JSON.stringify(await js(`(() => {const node=document.querySelector('[data-agent-id="${cardId}"]');return {dragged:node.dataset.wasDragged,hover:node.parentElement.matches(':hover'),order:window.__r06.store.getState().agents.map(item=>item.id)};})()`)));
  await until(`window.__r06.store.getState().agents[0].id !== '${cardId}'`, 'actual pointer reorder');
  window.hide();
  check('18-order-real-pointer-persist', (await api('GET', '/lingxi/v1/agents')).agents[0].id !== cardId);
  await selectAgent('agentA');
  await click('settings.memory.compiledView');
  await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.memory.editEntry') && !node.disabled)`, 'compiled clear after Dream restore load');
  await click('settings.memory.compiledClear');
  await until(`document.body.innerText.includes(window.t('settings.memory.compiledCleared'))`, 'real compiled clear success');
  const cleared = await api('GET', '/lingxi/v1/memories/compiled?agentId=agentA');
  check('11-compiled-clear-preserves-raw-facts-and-local-facts', (await api('GET','/lingxi/v1/memories?agentId=agentA')).memories.some(item => item.fact === 'Electron实际结构事实') && await js("document.body.innerText.includes('页面实际事实记忆')") && ['today','week','longterm'].every(key => cleared.sections[key] === ''));
  await closeViewer();
  await click('settings.memory.actions.clear'); await click('settings.memory.actions.confirmClear');
  await until(`document.body.innerText.includes(window.t('settings.memory.actions.clearSuccess'))`, 'whole memory actual clear finished');
  check('12-whole-memory-confirm-real-delete', (await api('GET', '/lingxi/v1/memories?agentId=agentA')).memories.length === 0);
  check('agent-B-isolated', fs.readFileSync(path.join(home, 'agents/agentB/config.yaml')).equals(savedB));
}

async function referencePending() {
  await selectAgent('agentA');
  await click('settings.agent.deleteBtn');
  await click('settings.agent.deleteNext');
  await until(`!!document.querySelector('input[placeholder="' + window.t('settings.agent.deletePlaceholder') + '"]') || Array.from(document.querySelectorAll('button')).some(node => node.textContent.trim() === window.t('settings.agent.deleteSkillsNextWithout'))`, 'actual cleanup preview');
  if (!await js(`!!document.querySelector('input[placeholder="' + window.t('settings.agent.deletePlaceholder') + '"]')`)) await click('settings.agent.deleteSkillsNextWithout');
  await input('input[placeholder="' + await js("window.t('settings.agent.deletePlaceholder')") + '"]', '助手agentA');
  await click('settings.agent.deleteConfirm');
  await until(`document.body.innerText.includes('助手引用更新待完成')`, 'actual pending reference visible');
  const receipts = await api('GET', '/lingxi/v1/agent-deletions');
  const receipt = receipts.deletions.find(row => row.agentId === 'agentA');
  check('reference-pending-real-receipt', receipt.logicalDeleted && receipt.referencesPending && receipt.cleanupPending);
  check('reference-pending-agent-absent', !fs.existsSync(path.join(home, 'agents/agentA')) && !(await api('GET', '/lingxi/v1/agents')).agents.some(agent => agent.id === 'agentA'));
  const receiptPath = path.join(home, 'lingxi-service/agent-deletions', receipt.deletionId, 'receipt.json');
  const validReceipt = fs.readFileSync(receiptPath);
  const corrupt = JSON.parse(validReceipt); corrupt.status = 'completed';
  fs.writeFileSync(receiptPath, JSON.stringify(corrupt));
  await js(`window.dispatchEvent(new Event('hana-agent-deletion-changed'))`);
  await until(`Array.from(document.querySelectorAll('[role=alert]')).some(node => node.innerText.includes('删除进度'))`, 'real invalid receipt GET failure visible');
  check('reference-read-failure-keeps-pending-visible', await js(`document.body.innerText.includes('助手引用更新待完成')`) && requests.some(request => request.path === '/lingxi/v1/agent-deletions' && request.status === 500));
  fs.writeFileSync(receiptPath, validReceipt);
  check('reference-pending-no-false-switch', await js(`window.__r06.store.getState().currentAgentId === null`));
  require('node:child_process').execFileSync('/usr/bin/chflags', ['nouchg', path.join(home, 'user/preferences.json')]);
  await js(`Array.from(document.querySelectorAll('button')).find(node => node.textContent.trim() === '重试清理').click()`);
  await until(`!document.querySelector('[aria-label="删除清理进度"]')`, 'real retry reconciles references and disk');
  const after = await api('GET', '/lingxi/v1/agent-deletions');
  check('reference-retry-completed', after.deletions[0].status === 'completed' && !after.deletions[0].cleanupPending && !after.deletions[0].referencesPending);
  const prefs = JSON.parse(fs.readFileSync(path.join(home, 'user/preferences.json'), 'utf8'));
  check('reference-retry-actual-prefs', prefs.current_agent_id === 'agentB' && prefs.primary_agent_id === 'agentB');
}

async function failures() {
  const child = require('node:child_process');
  const agentRoot = path.join(home, 'agents/agentA');
  const configFile = path.join(agentRoot, 'config.yaml');
  const originalConfig = fs.readFileSync(configFile);
  const config = () => api('GET', '/lingxi/v1/agents/agentA/config');
  await selectAgent('agentA');
  child.execFileSync('/usr/bin/chflags', ['uchg', configFile]);
  try {
    await toggle('settings.memory.sectionTitle');
    await until(`window.__r06.store.getState().toastType === 'error'`, 'real write failure toast');
    check('memory-io-failure-preserves-config', fs.readFileSync(configFile).equals(originalConfig) && (await config()).memory.enabled !== false);
    await input('select:has(option[value="embedding-stub/embedding-model"])', 'embedding-stub/embedding-model');
    await until(`document.querySelector('select:has(option[value="embedding-stub/embedding-model"])').value === '' && !document.querySelector('select:has(option[value="embedding-stub/embedding-model"])').disabled`, 'embedding failure rolls selector back');
    check('embedding-io-failure-no-false-save', !(await config()).memory.embedding_model?.provider);
    await toggle('settings.memory.dream.autoLabel');
    await until(`Array.from(document.querySelectorAll('[role=alert]')).some(node => node.innerText === window.t('settings.memory.dream.errors.autoSaveFailed'))`, 'Dream auto failure inline');
    check('dream-auto-io-failure-preserved', (await config()).memory.dream?.auto_enabled !== true);
    const mixedResponse = await fetch(url + '/lingxi/v1/agents/agentA/config', { method: 'PUT', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ locale: 'en', memory: { dream: { auto_enabled: true } } }) });
    check('mixed-patch-after-global-before-agent-fails', mixedResponse.status === 500);
    const prefs = JSON.parse(fs.readFileSync(path.join(home, 'user/preferences.json'), 'utf8'));
    check('mixed-patch-real-global-persisted-agent-not', prefs.locale === 'en' && fs.readFileSync(configFile).equals(originalConfig) && (await api('GET', '/lingxi/v1/settings/config')).locale === 'en');
  } finally { child.execFileSync('/usr/bin/chflags', ['nouchg', configFile]); }
  const preferencesFile = path.join(home, 'user/preferences.json');
  const previousWrites = requests.filter(request => request.path === '/lingxi/v1/agents/agentA/config' && request.method === 'PUT' && request.status === 200).length;
  cardNetwork.holdConfig = true;
  await js(`window.__r06.store.setState({toastVisible:false,toastMessage:'',toastType:'success'})`);
  await toggle('settings.memory.sectionTitle');
  for (let i=0; !cardNetwork.entered && i<500; i++) await new Promise(resolve => setTimeout(resolve, 10));
  assert.ok(cardNetwork.entered, 'actual config PUT response already arrived from production service');
  check('config-get-failure-does-not-imply-rollback', fs.readFileSync(configFile,'utf8').includes('enabled: false'));
  fs.renameSync(preferencesFile, preferencesFile + '.readback-saved'); fs.mkdirSync(preferencesFile);
  try {
    cardNetwork.release();
    await until(`window.__r06.store.getState().toastType === 'error'`, 'successful config write but actual preferences readback failed');
    check('config-put-success-get-failure-visible', requests.filter(request => request.path === '/lingxi/v1/agents/agentA/config' && request.method === 'PUT' && request.status === 200).length === previousWrites + 1 && requests.some(request => request.path === '/lingxi/v1/agents/agentA/config' && request.method === 'GET' && request.status === 500));
  } finally { fs.rmdirSync(preferencesFile); fs.renameSync(preferencesFile + '.readback-saved', preferencesFile); }
  await selectAgent('agentB'); await selectAgent('agentA');
  const behavior = path.join(agentRoot, 'AGENTS.md');
  const oldBehavior = fs.readFileSync(behavior);
  child.execFileSync('/usr/bin/chflags', ['uchg', behavior]);
  try {
    await js(`const nodes = Array.from(document.querySelectorAll('textarea')); nodes[0].id='r06-partial-identity'; nodes[1].id='r06-partial-behavior';`);
    await input('#r06-partial-identity', '身份确实部分写入'); await input('#r06-partial-behavior', '行为禁止写入');
    const priorToast = await js('window.__r06.store.getState().toastMessage');
    await js(`Array.from(document.querySelectorAll('button')).filter(node => node.textContent.trim() === window.t('settings.save')).at(-1).click()`);
    for (let poll = 0; poll < 200 && !requests.some(request => request.method === 'PUT' && request.path.endsWith('/agents-md') && request.status === 500); poll++) await new Promise(resolve => setTimeout(resolve, 10));
    check('persona-real-one-success-one-failure', requests.some(request => request.method === 'PUT' && request.path.endsWith('/identity') && request.status === 200) && requests.some(request => request.method === 'PUT' && request.path.endsWith('/agents-md') && request.status === 500));
    await until(`window.__r06.store.getState().toastType === 'error' && window.__r06.store.getState().toastMessage !== ${JSON.stringify(priorToast)}`, 'partial persona failure actual toast');
    check('persona-partial-write-honest', fs.readFileSync(path.join(agentRoot, 'identity.md'), 'utf8') === '身份确实部分写入' && fs.readFileSync(behavior).equals(oldBehavior));
  } finally { child.execFileSync('/usr/bin/chflags', ['nouchg', behavior]); }
  const factsPath = path.join(agentRoot, 'memory/facts.md');
  const factsBefore = fs.readFileSync(factsPath);
  fs.renameSync(factsPath, factsPath + '.fixture-saved'); fs.mkdirSync(factsPath);
  try {
    await click('settings.memory.compiledView');
    await until(`Array.from(document.querySelectorAll('[role=alert]')).some(node => node.innerText.includes('regular'))`, 'compiled failed read shown inline');
    check('compiled-read-error-is-not-empty-success', requests.some(request => request.path === '/lingxi/v1/memories/compiled' && request.status === 400));
    await js(`Array.from(document.querySelectorAll('button')).findLast(node => node.textContent.trim() === '✕').click()`);
  } finally { fs.rmdirSync(factsPath); fs.renameSync(factsPath + '.fixture-saved', factsPath); }
  check('compiled-failure-preserved-material', fs.readFileSync(factsPath).equals(factsBefore));
  const catalog = path.join(home, 'provider-catalog.json');
  fs.writeFileSync(catalog, '{broken actual catalog');
  try {
    await js(`window.dispatchEvent(new Event('hana-models-changed'))`);
    await until(`window.__r06.store.getState().runtimeModelsError !== null`, 'actual models failure state');
    check('models-failure-keeps-existing-list', await js(`window.__r06.store.getState().runtimeModels.length > 0 && document.querySelector('[role=alert]') !== null`));
  } finally { fs.unlinkSync(catalog); }
}

async function receiptOwner() {
  const http = require('node:http');
  const other = JSON.parse(fs.readFileSync(path.join(home, 'other-connection.json'), 'utf8'));
  let release; const held = new Promise(resolve => { release = resolve; });
  let entered = false; const forwarded = [];
  // 只延迟真实网络转发；后端仍是第二个生产服务，不制造业务响应。
  const proxy = http.createServer(async (request, response) => {
    forwarded.push({ method: request.method, path: request.url });
    if (request.method === 'GET' && request.url === '/lingxi/v1/agent-deletions') { entered = true; await held; }
    const target = new URL(request.url, other.url);
    const upstream = http.request(target, { method: request.method, headers: { ...request.headers, host: target.host } }, incoming => {
      response.writeHead(incoming.statusCode, incoming.headers); incoming.pipe(response);
    });
    upstream.on('error', error => response.destroy(error)); request.pipe(upstream);
  });
  await new Promise(resolve => proxy.listen(0, '127.0.0.1', resolve));
  const base = 'http://127.0.0.1:' + proxy.address().port;
  try {
    await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent === window.t('settings.agentDeletion.retry'))`, 'A pending receipt initially shown');
    check('receipt-owner-A-visible', await js(`document.body.innerText.includes('助手引用更新待完成')`));
    await js(`window.__r06.originalConnection = window.__r06.store.getState().serverConnections[window.__r06.store.getState().activeServerConnectionId]; const previous = window.__r06.originalConnection; window.__r06.store.setState({ serverConnections: {...window.__r06.store.getState().serverConnections, [previous.connectionId]: {...previous, baseUrl:${JSON.stringify(base)}, token:${JSON.stringify(other.token)}}} });`);
    for (let i=0; !entered && i<200; i++) await new Promise(resolve => setTimeout(resolve, 10));
    assert.ok(entered, 'B genuine receipt GET is held in real network');
    const staleVisible = await js(`Array.from(document.querySelectorAll('button')).some(node => node.textContent === window.t('settings.agentDeletion.retry'))`);
    await js(`Array.from(document.querySelectorAll('button')).find(node => node.textContent === window.t('settings.agentDeletion.retry'))?.click()`);
    await new Promise(resolve => setTimeout(resolve, 200));
    fs.writeFileSync(path.join(home, 'receipt-owner-counterexample.json'), JSON.stringify({ staleVisible, forwarded }, null, 2));
    check('receipt-owner-B-loading-hides-A', !staleVisible);
    check('receipt-owner-B-zero-cleanup', !forwarded.some(request => request.method === 'POST' && request.path.endsWith('/cleanup')));
    await js(`const original=window.__r06.originalConnection; window.__r06.store.setState({serverConnections:{...window.__r06.store.getState().serverConnections,[original.connectionId]:original}});`);
    await until(`Array.from(document.querySelectorAll('button')).some(node => node.textContent === window.t('settings.agentDeletion.retry'))`, 'return A requires actual A receipt reload');
    check('receipt-owner-return-A-reloaded', requests.filter(request => request.path === '/lingxi/v1/agent-deletions' && request.status === 200).length >= 2);
    release(); await new Promise(resolve => setTimeout(resolve, 150));
    check('receipt-owner-late-B-cannot-replace-A', await js(`document.body.innerText.includes('助手引用更新待完成')`) && !forwarded.some(request => request.method === 'POST'));
  } finally { release(); proxy.closeAllConnections(); await new Promise(resolve => proxy.close(resolve)); }
}

async function prepareCardNetwork() {
  const http = require('node:http');
  let release; const waiting = new Promise(resolve => { release = resolve; });
  const state = { base: '', entered: false, release, forwarded: [], server: null, holds: new Map() };
  state.pause = route => { let resume; const promise=new Promise(resolve=>{resume=resolve}); const hold={entered:false,release:()=>{state.holds.delete(route);resume()},promise}; state.holds.set(route,hold); return hold; };
  if (scenario==='initialization') state.summaryHold=state.pause('/lingxi/v1/providers/summary');
  state.server = http.createServer(async (request, response) => {
    state.forwarded.push({ method: request.method, path: request.url });
    const hold=state.holds.get(request.url.split('?')[0]); if (request.method==='GET' && hold) {hold.entered=true;await hold.promise;}
    const target = new URL(request.url, url);
    const upstream = http.request(target, { method: request.method, headers: { ...request.headers, host: target.host } }, async incoming => {
      // 已有真实 200 和写盘结果，只延迟这次网络响应，让用户能在等待中切换服务。
      if ((request.method === 'POST' && request.url === '/lingxi/v1/character-cards/import') || (state.holdTenet && request.method==='POST' && request.url==='/lingxi/v1/agents/agentA/tenets') || (state.holdConfig && request.method === 'PUT' && request.url === '/lingxi/v1/agents/agentA/config')) { state.holdConfig = false; state.holdTenet=false; state.entered = true; await waiting; }
      response.writeHead(incoming.statusCode, incoming.headers); incoming.pipe(response);
    });
    upstream.on('error', error => response.destroy(error)); request.pipe(upstream);
  });
  await new Promise(resolve => state.server.listen(0, '127.0.0.1', resolve));
  state.base = 'http://127.0.0.1:' + state.server.address().port;
  return state;
}
async function cardOwner() {
  const other = JSON.parse(fs.readFileSync(path.join(home, 'other-connection.json'), 'utf8'));
  await js(`window.__r06.originalConnection=window.__r06.store.getState().serverConnections[window.__r06.store.getState().activeServerConnectionId]`);
  const connection = async otherSide => js(`(() => { const original=window.__r06.originalConnection; const next=${otherSide ? `{...original,baseUrl:${JSON.stringify(other.url)},token:${JSON.stringify(other.token)}}` : 'original'}; window.__r06.store.setState({serverConnections:{...window.__r06.store.getState().serverConnections,[original.connectionId]:next}}); })()`);
  const noPreview = async () => until(`!Array.from(document.querySelectorAll('button')).some(node => node.textContent === window.t('settings.characterCard.confirm'))`, 'old connection preview hidden');
  const ready = async () => until(`window.__r06.store.getState().settingsConfigStatus === 'ready' && !!document.querySelector('select[aria-label=助手]') && !document.querySelector('select[aria-label=助手]').disabled`, 'real connection settings readback');
  await click('settings.agent.exportAgent');
  await until(`!!document.querySelector('[role=dialog] input[type=checkbox]')`, 'A actual export preview');
  await connection(true); await ready();
  const staleExport = await js(`Array.from(document.querySelectorAll('button')).some(node => node.textContent === window.t('settings.characterCard.confirm'))`);
  if (staleExport) { await js(`Array.from(document.querySelectorAll('button')).find(node => node.textContent === window.t('settings.characterCard.confirm')).click()`); await new Promise(resolve => setTimeout(resolve,300)); }
  fs.writeFileSync(path.join(home, 'card-export-owner-counterexample.json'), JSON.stringify({staleExport,requests},null,2));
  check('card-owner-export-preview-hidden-on-B', !staleExport && !requests.some(request => request.origin === other.url && request.method === 'POST' && request.path === '/lingxi/v1/character-cards/export'));
  await connection(false); await ready(); await noPreview();
  check('card-owner-export-away-back-invalid', true);
  // 新的导入规划必须由当前服务真正读取文件，旧 token 不能跨服务确认。
  const openImport = async () => {
    await js(`document.querySelector('[data-add-btn="1"]').click()`);
    await until(`!!document.querySelector('input[type=file][accept*=hana-package]')`, 'actual import input');
    await fileInput('input[type=file][accept*=hana-package]', path.join(home, '..', 'workspace', 'owner-card.json'));
    await until(`!!document.querySelector('[role=dialog] input[type=checkbox]:not(:disabled)')`, 'A actual import planning');
  };
  await openImport(); await connection(true); await noPreview();
  check('card-owner-import-preview-hidden-on-B', !requests.some(request => request.origin === other.url && request.method === 'POST' && request.path === '/lingxi/v1/character-cards/import'));
  await connection(false); await ready(); await noPreview();
  check('card-owner-import-away-back-invalid', true);
  await openImport(); await click('settings.characterCard.confirm');
  for (let i=0; !cardNetwork.entered && i<1000; i++) await new Promise(resolve => setTimeout(resolve, 10));
  assert.ok(cardNetwork.entered, 'real A import 200 response is delayed');
  await connection(true); await ready(); cardNetwork.release();
  await new Promise(resolve => setTimeout(resolve, 300));
  fs.writeFileSync(path.join(home,'card-import-owner-counterexample.json'),JSON.stringify({requests,currentAgentId:await js('window.__r06.store.getState().currentAgentId')},null,2));
  check('card-owner-import-completed-only-on-A', fs.existsSync(path.join(home, 'agents/card-from-a/config.yaml')));
  check('card-owner-no-import-or-switch-to-B', !requests.some(request => request.origin === other.url && request.method === 'POST' && ['/lingxi/v1/character-cards/import','/lingxi/v1/character-cards/export','/lingxi/v1/agents/switch'].includes(request.path)));
  check('card-owner-no-old-created-projection-on-B', await js(`window.__r06.store.getState().currentAgentId !== 'card-from-a'`));
  await connection(false); await ready();
  check('card-owner-completed-response-stays-invalid-after-return', !(await js(`window.__r06.store.getState().toastMessage.includes('已创建')`)));
  cardNetwork.server.closeAllConnections(); await new Promise(resolve => cardNetwork.server.close(resolve));
}

async function initializationBoundaries() {
  const summary=cardNetwork.summaryHold;
  for(let i=0;!summary.entered&&i<200;i++)await new Promise(resolve=>setTimeout(resolve,10));
  check('00-slow-summary-does-not-block-ready', summary.entered && await js('window.__r06.store.getState().ready') && !requests.some(request=>request.path==='/lingxi/v1/providers/summary'&&request.method==='GET'));
  const catalog=path.join(home,'provider-catalog.json'); const old=fs.existsSync(catalog)?fs.readFileSync(catalog):null;
  fs.writeFileSync(catalog,'{invalid actual provider catalog'); summary.release();
  try {
    await until(`!!window.__r06.store.getState().providersSummaryError`, 'actual provider summary failure marked');
    check('00-summary-failure-explicit-not-empty-success', await js('window.__r06.store.getState().ready && !!window.__r06.store.getState().providersSummaryError') && requests.some(request=>request.path==='/lingxi/v1/providers/summary'&&request.status===500));
  } finally { if(old)fs.writeFileSync(catalog,old);else fs.unlinkSync(catalog); }
  const previousName=await js(`document.querySelector('input[class*=agent-name-input]').value`);
  const snapshotsBefore=requests.filter(request=>request.path==='/lingxi/v1/settings/snapshot'&&request.method==='GET').length;
  const identity=cardNetwork.pause('/lingxi/v1/server/identity');
  await js('window.r06SettingsUnmount();window.r06SettingsMount()');
  await until(`window.__r06.store.getState().ready && !document.querySelector('#settingsLoadingMask') && document.querySelector('input[class*=agent-name-input]')?.value===${JSON.stringify(previousName)}`, 'same owner cached reopen stays visible before real identity response');
  check('00-same-owner-cache-reopen-no-mask', await js(`window.__r06.store.getState().ready && !document.querySelector('#settingsLoadingMask') && document.querySelector('input[class*=agent-name-input]').value===${JSON.stringify(previousName)}`));
  identity.release();
  for(let i=0;requests.filter(request=>request.path==='/lingxi/v1/settings/snapshot'&&request.method==='GET').length<=snapshotsBefore&&i<500;i++)await new Promise(resolve=>setTimeout(resolve,10));
  await until(`!!document.querySelector('select[aria-label=助手]') && !document.querySelector('select[aria-label=助手]').disabled && window.__r06.store.getState().settingsConfigStatus==='ready' && window.__r06.store.getState().settingsSnapshot.status==='ready'`, 'cached refresh returns actual data');
  await js('window.r06SettingsUnmount();window.__r06.store.setState({ready:false,settingsConfig:null,settingsDataOwner:null,settingsDataAgentId:null,agents:[],currentAgentId:null,settingsAgentId:null,globalModelsConfig:null});');
  const cold=cardNetwork.pause('/lingxi/v1/server/identity'); const started=Date.now();
  await js('window.r06SettingsMount()');
  await until(`!!document.querySelector('#settingsLoadingMask')`, 'cold actual loading mask');
  check('00-cold-mask-before-forced-ready', !(await js('window.__r06.store.getState().ready')));
  await until('window.__r06.store.getState().ready', 'real fifteen second forced ready',18000);
  check('00-real-fifteen-second-forced-ready', Date.now()-started>=14900 && !await js(`!!document.querySelector('#settingsLoadingMask')`) && !await js(`!!document.querySelector('input[class*=agent-name-input]')`));
  cold.release(); await until(`window.__r06.store.getState().settingsConfigStatus==='ready' && !!document.querySelector('input[class*=agent-name-input]')`, 'real late initialization completion');
  check('00-forced-ready-does-not-fake-loaded-data', await js(`window.__r06.store.getState().settingsDataAgentId==='agentA' && window.__r06.store.getState().settingsConfig.agent.name===${JSON.stringify(previousName)}`));
}
async function remainingBoundaries() {
  await until(`document.body.innerText.includes('已有真实原则')`, 'real original tenet loaded');
  cardNetwork.holdTenet=true;
  await input('input[maxlength="300"]','新写入但刷新失败');
  await js(`document.querySelector('input[maxlength="300"]').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))`);
  for(let i=0;!cardNetwork.entered&&i<500;i++)await new Promise(resolve=>setTimeout(resolve,10));
  assert.ok(cardNetwork.entered,'actual tenet POST response arrived');
  const tenetPath=path.join(home,'agents/agentA/memory/tenets.json');const written=fs.readFileSync(tenetPath);
  check('14-write-persisted-before-refresh-error', JSON.parse(written).tenets.some(item=>item.content==='新写入但刷新失败'));
  fs.writeFileSync(tenetPath,'{invalid real tenet file');cardNetwork.release();
  try {
    await until(`document.body.innerText.includes('tenets') || document.body.innerText.includes('原则')`, 'tenet real failed refresh');
    for(let i=0;!requests.some(request=>request.method==='GET'&&request.path==='/lingxi/v1/agents/agentA/tenets'&&request.status>=400)&&i<500;i++)await new Promise(resolve=>setTimeout(resolve,10));
    check('14-success-post-failed-get-old-list-and-error', requests.some(request=>request.method==='POST'&&request.path==='/lingxi/v1/agents/agentA/tenets'&&request.status===200) && requests.some(request=>request.method==='GET'&&request.path==='/lingxi/v1/agents/agentA/tenets'&&request.status>=400) && await js(`document.body.innerText.includes('已有真实原则') && !Array.from(document.querySelectorAll('span')).some(node=>node.textContent==='新写入但刷新失败') && !!document.querySelector('[class*=tenets-error]')`));
  } finally {fs.writeFileSync(tenetPath,written);}
  check('14-readback-confirms-no-fake-rollback', (await api('GET','/lingxi/v1/agents/agentA/tenets')).tenets.some(item=>item.content==='新写入但刷新失败'));
  const healthCount=()=>requests.filter(request=>request.method==='GET'&&request.path==='/lingxi/v1/memories/health').length;
  const before=healthCount();await toggle('settings.memory.sectionTitle');
  await until(`window.__r06.store.getState().settingsConfig.memory.enabled===false && !document.querySelector('[class*=memory-health-strip]')`, 'memory prerequisite clears health');
  await new Promise(resolve=>setTimeout(resolve,100));check('17-disabled-clears-old-health-and-zero-get',healthCount()===before);
  const hold=cardNetwork.pause('/lingxi/v1/memories/health');await toggle('settings.memory.sectionTitle');
  for(let i=0;!hold.entered&&i<500;i++)await new Promise(resolve=>setTimeout(resolve,10));assert.ok(hold.entered,'actual health request held');
  const configPath=path.join(home,'agents/agentA/config.yaml');const good=fs.readFileSync(configPath);fs.writeFileSync(configPath,'memory: [invalid');hold.release();
  try {await until(`document.querySelector('[class*=memory-health-strip]')?.innerText.includes(window.t('settings.memory.health.unavailable'))`, 'actual health read error unavailable');
    check('17-actual-read-error-unavailable',requests.some(request=>request.path==='/lingxi/v1/memories/health'&&request.status>=400)&&await js(`document.querySelector('[class*=memory-health-strip]').innerText.includes(window.t('settings.memory.health.unavailable'))`));
  } finally {fs.writeFileSync(configPath,good);}
  const beforeOff=healthCount();await toggle('settings.memory.sectionTitle');await until(`!document.querySelector('[class*=memory-health-strip]')`,'failed health cleared by disabled prerequisite');await new Promise(resolve=>setTimeout(resolve,100));
  check('17-disabled-clears-error-and-zero-get',healthCount()===beforeOff);
  await toggle('settings.memory.sectionTitle');await until(`document.querySelector('[class*=memory-health-strip]')?.innerText.includes('daily_pipeline_absent_in_candidate')`,'restored real health');
  const order=(await api('GET','/lingxi/v1/agents')).agents.map(agent=>agent.id);const id=order[0];
  window.show();app.focus({steal:true});window.focus();await new Promise(resolve=>setTimeout(resolve,200));
  const target=async()=>{await js(`document.querySelector('[data-agent-id="${id}"]').scrollIntoView({block:'center'})`);const rect=await js(`(()=>{const r=document.querySelector('[data-agent-id="${id}"]').getBoundingClientRect();return {x:r.x,y:r.y}})()`);window.webContents.sendInputEvent({type:'mouseMove',x:Math.round(rect.x+30),y:Math.round(rect.y+30)});await new Promise(resolve=>setTimeout(resolve,400));return js(`(()=>{const box=document.querySelector('[data-agent-id="${id}"]').getBoundingClientRect();for(let y=Math.max(0,box.top+5);y<Math.min(innerHeight,box.bottom-5);y+=5)for(let x=Math.max(0,box.left+5);x<Math.min(innerWidth,box.right-5);x+=5)if(document.elementFromPoint(x,y)?.closest('[data-agent-id]')?.dataset.agentId==='${id}')return {x,y};return null})()`)};
  let point=await target();assert.ok(point);const orderCount=requests.filter(request=>request.path==='/lingxi/v1/agents/order'&&request.method==='PUT').length;
  await js(`document.querySelector('[data-agent-id="${id}"]').addEventListener('pointercancel',event=>{window.r06Cancelled={trusted:event.isTrusted,type:event.type}},{once:true})`);
  await window.webContents.debugger.sendCommand('Emulation.setTouchEmulationEnabled',{enabled:true});
  await window.webContents.debugger.sendCommand('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:point.x,y:point.y,id:1}]});
  await window.webContents.debugger.sendCommand('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:point.x+160,y:point.y,id:1}]});
  await window.webContents.debugger.sendCommand('Input.dispatchTouchEvent',{type:'touchCancel',touchPoints:[]});
  await window.webContents.debugger.sendCommand('Emulation.setTouchEmulationEnabled',{enabled:false});await new Promise(resolve=>setTimeout(resolve,120));
  check('18-native-trusted-pointercancel-zero-order-put',await js(`window.r06Cancelled?.trusted===true`)&&requests.filter(request=>request.path==='/lingxi/v1/agents/order'&&request.method==='PUT').length===orderCount);
  check('18-pointercancel-keeps-real-order',JSON.stringify((await api('GET','/lingxi/v1/agents')).agents.map(agent=>agent.id))===JSON.stringify(order));
  await api('PUT','/lingxi/v1/agents/primary',{id:'agentA'});const prefs=path.join(home,'user/preferences.json');const bytes=fs.readFileSync(prefs);require('node:child_process').execFileSync('/usr/bin/chflags',['uchg',prefs]);
  const reads=requests.filter(request=>request.path==='/lingxi/v1/agents'&&request.method==='GET').length;
  try {point=await target();assert.ok(point);window.webContents.sendInputEvent({type:'mouseDown',x:Math.round(point.x),y:Math.round(point.y),button:'left',clickCount:1});window.webContents.sendInputEvent({type:'mouseMove',x:Math.round(point.x+160),y:Math.round(point.y),modifiers:['leftButtonDown']});await new Promise(resolve=>setTimeout(resolve,100));window.webContents.sendInputEvent({type:'mouseUp',x:Math.round(point.x+160),y:Math.round(point.y),button:'left',clickCount:1});
    await until(`window.__r06.store.getState().agents.map(agent=>agent.id).join(',')===${JSON.stringify(order.join(','))}`,'real failed order recovery');
    for(let i=0;!requests.some(request=>request.path==='/lingxi/v1/agents/order'&&request.status>=400)&&i<500;i++)await new Promise(resolve=>setTimeout(resolve,10));
    for(let i=0;requests.filter(request=>request.path==='/lingxi/v1/agents'&&request.method==='GET').length<=reads&&i<500;i++)await new Promise(resolve=>setTimeout(resolve,10));
    check('18-order-real-io-failure-then-get-recovery',requests.some(request=>request.path==='/lingxi/v1/agents/order'&&request.method==='PUT'&&request.status===500)&&requests.filter(request=>request.path==='/lingxi/v1/agents'&&request.method==='GET').length>reads&&await js(`window.__r06.store.getState().agents.map(agent=>agent.id).join(',')===${JSON.stringify(order.join(','))}`));
    check('18-order-failure-preserves-real-prefs',fs.readFileSync(prefs).equals(bytes)&&JSON.stringify((await api('GET','/lingxi/v1/agents')).agents.map(agent=>agent.id))===JSON.stringify(order));
  } finally {require('node:child_process').execFileSync('/usr/bin/chflags',['nouchg',prefs]);window.hide();}
}
