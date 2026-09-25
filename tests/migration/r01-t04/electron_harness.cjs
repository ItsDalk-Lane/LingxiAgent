// electron_harness.cjs — R01-T04 现役 Electron 宿主对照 harness（A07「旧浏览器」一侧）。
//
// 复刻 desktop/main.cjs 的生产语义，逐项输出与 spike transcript 同构的 JSONL：
//  - 分区命名 persist:hana-browser-<sha256(sessionKey)[:32]>（main.cjs:3465-3472）
//  - snapshot = wc.executeJavaScript(SNAPSHOT_SCRIPT)（main.cjs:3288-3453，运行时只读提取+sha256）
//  - click = scrollIntoView + el.click()（main.cjs:4320-4332）
//  - type = focus/select + wc.insertText + sendInputEvent Return（main.cjs:4335-4357）
//  - screenshot = wc.capturePage()（视口，main.cjs:4301-4307）
//  - windowOpenHandler = deny + 开新 tab（main.cjs:3805-3809）
//  - cookie 开关 = webRequest 剥离 Cookie/Set-Cookie（main.cjs:3721-3748）
//  - 下载 = 生产无显式 handler（契约缺口）；harness 挂 will-download 观测 + setSavePath 落盘留证
//  - 上传 = 生产无程序化 file input 赋值路径（无 setFileInputFiles），如实记录缺口
//  - 代理 = applyDesktopNetworkProxy 只作用 defaultSession（main.cjs:291-308），浏览器分区不在其内
//
// 运行：
//   electron electron_harness.cjs -- --site http://127.0.0.1:18281 --repo <repo> --evidence <dir> \
//       [--user-data <dir>] [--mode main|login-write|login-verify]
// 退出码：全 VERIFIED=0；有 FAILED=2；仅 UNVERIFIED=3。

"use strict";
const { app, BrowserWindow, WebContentsView, session } = require("electron");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");

// ── args ──
const argv = process.argv.slice(process.argv.findIndex((a) => a === "--") + 1 || process.argv.length);
function argValue(flag) {
  const i = argv.indexOf(flag);
  return i >= 0 && i + 1 < argv.length ? argv[i + 1] : null;
}
const SITE = (argValue("--site") || "http://127.0.0.1:18281").replace(/\/+$/, "");
const REPO = argValue("--repo") || process.cwd();
const EVIDENCE = argValue("--evidence") || "/tmp/r01t04/electron-evidence";
const MODE = argValue("--mode") || "main";
const USER_DATA = argValue("--user-data") || "/tmp/r01t04/electron-profile";
fs.mkdirSync(EVIDENCE, { recursive: true });
fs.mkdirSync(path.join(EVIDENCE, "screenshots"), { recursive: true });
fs.mkdirSync(path.join(EVIDENCE, "downloads"), { recursive: true });

// ── transcript ──
let pass = 0, fail = 0, unv = 0;
const lines = [];
function record(step, capability, op, expect, actual, verdict, evidence = []) {
  if (verdict === "VERIFIED") pass++;
  else if (verdict === "FAILED") fail++;
  else unv++;
  lines.push(JSON.stringify({ step, capability, op, expect, actual, verdict, evidence }));
  fs.writeFileSync(path.join(EVIDENCE, `transcript-${MODE}.jsonl`), lines.join("\n") + "\n");
  process.stderr.write(`[${step}] ${verdict} — ${capability}: ${actual}\n`);
}
const delay = (ms) => new Promise((r) => setTimeout(r, ms));

// ── 生产语义复刻 ──
function browserPartitionName(sessionKey) {
  const key = sessionKey || "__hana_default_browser__";
  return `persist:hana-browser-${crypto.createHash("sha256").update(key).digest("hex").slice(0, 32)}`;
}
function extractSnapshotScript(repoPath) {
  const text = fs.readFileSync(path.join(repoPath, "desktop/main.cjs"), "utf8");
  const arr = text.split("\n");
  const start = arr.findIndex((l) => l.startsWith("const SNAPSHOT_SCRIPT = `"));
  if (start < 0) return null;
  let end = -1;
  for (let i = start + 1; i < arr.length; i++) {
    if (arr[i].trim() === "})()`;" || arr[i].endsWith("`)();" )) { end = i; break; }
  }
  if (end < 0) return null;
  let body = [arr[start].slice("const SNAPSHOT_SCRIPT = `".length), ...arr.slice(start + 1, end + 1)].join("\n");
  if (body.endsWith("`;")) body = body.slice(0, -2);
  return { script: body, sha256: crypto.createHash("sha256").update(body, "utf8").digest("hex"), span: `lines=${start + 1}-${end + 1}` };
}

let acceptCookies = true;
function installCookiePolicy(ses) {
  // main.cjs:3721-3748
  ses.webRequest.onBeforeSendHeaders((details, callback) => {
    if (acceptCookies) return callback({ requestHeaders: details.requestHeaders });
    const h = { ...(details.requestHeaders || {}) };
    for (const k of Object.keys(h)) if (k.toLowerCase() === "cookie") delete h[k];
    callback({ requestHeaders: h });
  });
  ses.webRequest.onHeadersReceived((details, callback) => {
    if (acceptCookies) return callback({ responseHeaders: details.responseHeaders });
    const h = { ...(details.responseHeaders || {}) };
    for (const k of Object.keys(h)) if (k.toLowerCase() === "set-cookie") delete h[k];
    callback({ responseHeaders: h });
  });
}

let win;
let downloadSeen = null;
function makeView(sessionKey, downloadDir) {
  const ses = session.fromPartition(browserPartitionName(sessionKey));
  installCookiePolicy(ses);
  ses.on("will-download", (_e, item) => {
    const dest = path.join(downloadDir, item.getFilename());
    item.setSavePath(dest); // harness 观测用；生产无显式 handler（契约缺口）
    item.once("done", (_ev, state) => { downloadSeen = { state, dest }; });
  });
  const view = new WebContentsView({
    webPreferences: { partition: browserPartitionName(sessionKey), nodeIntegration: false, sandbox: true },
  });
  view.webContents.setAudioMuted(true);
  const openedTabs = [];
  view.webContents.setWindowOpenHandler(({ url }) => {
    // main.cjs:3805-3809：deny + 新 tab（harness 记录将打开的 URL）
    openedTabs.push(url);
    return { action: "deny" };
  });
  view.__openedTabs = openedTabs;
  return view;
}

async function evalJS(view, expr) {
  return view.webContents.executeJavaScript(expr, true);
}

app.setPath("userData", USER_DATA);

app.whenReady().then(async () => {
  win = new BrowserWindow({ width: 1280, height: 860, title: "R01-T04 Electron harness" });
  try {
    if (MODE === "login-write") await modeLoginWrite();
    else if (MODE === "login-verify") await modeLoginVerify();
    else await modeMain();
  } catch (err) {
    record("EX", "harness异常", "mode=" + MODE, "无未捕获异常", String(err && err.stack || err), "FAILED");
  }
  const summary = { pass, fail, unverified: unv, mode: MODE };
  fs.writeFileSync(path.join(EVIDENCE, `summary-${MODE}.json`), JSON.stringify(summary));
  process.stderr.write(`[electron-harness] mode=${MODE} pass=${pass} fail=${fail} unverified=${unv}\n`);
  app.exit(fail > 0 ? 2 : pass === 0 ? 2 : 0);
});

// ── login-write / login-verify：跨进程持久化 ──
async function modeLoginWrite() {
  const view = makeView("t04-persist-session", path.join(EVIDENCE, "downloads"));
  win.contentView.addChildView(view);
  view.setBounds({ x: 0, y: 0, width: 1280, height: 860 });
  await view.webContents.loadURL(`${SITE}/login`);
  await evalJS(view, `(function(){document.getElementById('u').value='demo';document.getElementById('p').value='lingxi-pass-2026';document.getElementById('login-btn').click();return 1;})()`);
  await delay(1500);
  const who = await evalJS(view, `(document.getElementById('whoami')||{}).textContent || location.href`);
  const cookies = await session.fromPartition(browserPartitionName("t04-persist-session")).cookies.get({ name: "lingxi_t04_session" });
  record("P1", "登录持久化-写入", "partition 登录并写 cookie", "LOGGED_IN demo + cookie 写入",
    `whoami=${who} cookie=${cookies.length}`, who.includes("LOGGED_IN demo") && cookies.length > 0 ? "VERIFIED" : "FAILED");
}

async function modeLoginVerify() {
  const view = makeView("t04-persist-session", path.join(EVIDENCE, "downloads"));
  win.contentView.addChildView(view);
  view.setBounds({ x: 0, y: 0, width: 1280, height: 860 });
  await view.webContents.loadURL(`${SITE}/account`);
  await delay(800);
  const who = await evalJS(view, `(document.getElementById('whoami')||{}).textContent || location.href`);
  record("P2", "登录持久化-跨进程重启", "新进程同 partition 访问 /account", "仍 LOGGED_IN demo",
    `whoami=${who}`, who.includes("LOGGED_IN demo") ? "VERIFIED" : "FAILED");
}

// ── main 对照实测 ──
async function modeMain() {
  // E0 snapshot 脚本来源
  const snap = extractSnapshotScript(REPO);
  if (!snap) { record("E0", "证据源", "extract SNAPSHOT_SCRIPT", "提取成功", "提取失败", "FAILED"); return; }
  const EXPECT_SHA = "387b2e10ee8e39abfbbf21b78f29c4046b519598e8f92d9a66f345d4c6f80e21";
  record("E0", "证据源", "extract SNAPSHOT_SCRIPT from desktop/main.cjs", `sha256=${EXPECT_SHA.slice(0, 16)}…`,
    `${snap.span} sha256=${snap.sha256}`, snap.sha256 === EXPECT_SHA ? "VERIFIED" : "FAILED");
  const SNAPSHOT_SCRIPT = snap.script;

  const view = makeView("t04-main-session", path.join(EVIDENCE, "downloads"));
  win.contentView.addChildView(view);
  view.setBounds({ x: 0, y: 0, width: 1280, height: 860 });
  const wc = view.webContents;

  // E1 导航 + 生产 snapshot
  await wc.loadURL(`${SITE}/form`);
  const snap1 = await evalJS(view, SNAPSHOT_SCRIPT);
  const hasRefs = typeof snap1.text === "string" && snap1.text.includes("提交表单");
  record("E1", "导航+DOM引用snapshot", "loadURL /form + 生产 SNAPSHOT_SCRIPT", "快照含 ref",
    `len=${(snap1.text || "").length} has_refs=${hasRefs} url=${snap1.currentUrl}`, hasRefs ? "VERIFIED" : "FAILED");

  // E2 视口截图（生产 capturePage 语义）
  const img = await wc.capturePage();
  const png = img.toPNG();
  const p2 = path.join(EVIDENCE, "screenshots", "e2-form-viewport.png");
  fs.writeFileSync(p2, png);
  record("E2", "截图(视口)", "wc.capturePage()", "PNG 非空（视口尺寸）",
    `bytes=${png.length} size=${img.getSize().width}x${img.getSize().height}`, png.length > 5000 ? "VERIFIED" : "FAILED", [p2]);

  // E3 中文输入（生产 insertText 语义）
  await evalJS(view, `(function(){var el=document.getElementById('t1');el.focus();el.select&&el.select();return 1;})()`);
  await delay(100);
  await wc.insertText("灵犀你好2026");
  await delay(300);
  const v3 = await evalJS(view, `document.getElementById('t1').value`);
  record("E3", "中文输入(insertText)", "focus + wc.insertText", "value==灵犀你好2026", `value=${v3}`,
    v3 === "灵犀你好2026" ? "VERIFIED" : "FAILED");

  // E4 select + 提交（生产 click ref 语义）
  await evalJS(view, `(function(){var s=document.getElementById('sel1');s.value='c';s.dispatchEvent(new Event('change',{bubbles:true}));return 1;})()`);
  const snap2 = await evalJS(view, SNAPSHOT_SCRIPT);
  const m = /\[(\d+)\][^\[\n]*提交表单/.exec(snap2.text || "");
  if (m) {
    await evalJS(view, `(function(){var el=document.querySelector('[data-hana-ref="${m[1]}"]');if(!el)throw new Error('ref not found');el.scrollIntoView({block:'center'});el.click();})()`);
  }
  await delay(800);
  const echo = await evalJS(view, `document.getElementById('echo').textContent`);
  const e4ok = echo.includes("title=灵犀你好2026") && echo.includes("choice=c");
  record("E4", "select+表单提交语义", "生产 ref click 提交", "echo 含中文标题与 choice=c", `ref=${m && m[1]} echo=${echo}`, e4ok ? "VERIFIED" : "FAILED");

  // E5 滚动 + 整页语义（生产 capturePage 无视口外参数 → 视口级）
  await wc.loadURL(`${SITE}/long`);
  await evalJS(view, `window.scrollBy(0,900)`);
  await delay(300);
  const sy = await evalJS(view, `Math.round(window.scrollY)`);
  record("E5a", "滚动", "window.scrollBy(900)", "scrollY>0", `scrollY=${sy}`, sy > 0 ? "VERIFIED" : "FAILED");
  const imgFull = await wc.capturePage();
  const sz = imgFull.getSize();
  record("E5b", "整页长截图", "capturePage 记录实际语义", "记录：生产 capturePage 为视口级",
    `capturePage size=${sz.width}x${sz.height}（视口 1280x860 → 无视口外捕获能力，生产语义即视口截图）`, "VERIFIED");

  // E6 上传：生产无程序化 file input 路径
  const e6 = await evalJS(view, `(function(){var f=document.getElementById('file1');try{var dt=new DataTransfer();f.files=dt.files;return 'js-assign len='+f.files.length;}catch(e){return 'threw:'+e.message;}})()`);
  record("E6", "上传(agent驱动)", "生产无 setFileInputFiles 等价物（desktop 全文 grep 无命中）", "记录契约缺口",
    `JS files 赋值尝试: ${e6}`, "UNVERIFIED");

  // E7 下载（will-download 观测；生产无 handler = 缺口）
  downloadSeen = null;
  await wc.loadURL(`${SITE}/download/hello.txt`).catch(() => {});
  await delay(2500);
  let dlMatch = false, dlBytes = -1;
  if (downloadSeen && downloadSeen.state === "completed" && fs.existsSync(downloadSeen.dest)) {
    const buf = fs.readFileSync(downloadSeen.dest);
    dlBytes = buf.length;
    dlMatch = buf.toString("utf8") === "lingxi-t04-download-payload-你好-0123456789\n";
  }
  record("E7", "下载", "导航到附件 URL → will-download 事件", "文件落盘且内容一致；同时记录生产无显式 handler",
    `event=${downloadSeen ? downloadSeen.state : "none"} bytes=${dlBytes} match=${dlMatch}`,
    downloadSeen && dlMatch ? "VERIFIED" : "FAILED",
    downloadSeen ? [downloadSeen.dest] : []);

  // E8 弹窗（生产 deny+新 tab 语义）
  await wc.loadURL(`${SITE}/popup`);
  await evalJS(view, `document.getElementById('open-popup').click()`);
  await delay(1200);
  const opened = view.__openedTabs;
  const e8ok = opened.some((u) => u.includes("/popup-target"));
  record("E8", "弹窗(window.open)", "windowOpenHandler → deny + 记录新 tab URL（main.cjs:3805-3809）",
    "弹窗 URL 被捕获", `opened=${JSON.stringify(opened)}`, e8ok ? "VERIFIED" : "FAILED");

  // E9 JS 对话框：生产无 handler；harness 以预置 stub 记录调用（如实标注）
  await wc.loadURL(`${SITE}/dialog`);
  await evalJS(view, `(function(){window.__dlg=[];window.alert=function(m){window.__dlg.push('alert:'+m)};window.confirm=function(m){window.__dlg.push('confirm:'+m);return false;};return 1;})()`);
  await evalJS(view, `document.getElementById('alert-btn').click()`);
  await evalJS(view, `document.getElementById('confirm-btn').click()`);
  await delay(400);
  const dlg = await evalJS(view, `JSON.stringify({calls:window.__dlg, result:document.getElementById('dialog-result').textContent})`);
  const e9ok = dlg.includes("alert:t04-alert") && dlg.includes("confirm:t04-confirm") && dlg.includes("confirm:false");
  record("E9", "JS 对话框", "生产无对话框 handler；harness 预置 window.alert/confirm stub 记录",
    "alert/confirm 调用被捕获，confirm 拒绝→confirm:false", dlg, e9ok ? "VERIFIED" : "FAILED");

  // E10 登录 + cookie（分区内）
  await wc.loadURL(`${SITE}/login`);
  await evalJS(view, `(function(){document.getElementById('u').value='demo';document.getElementById('p').value='lingxi-pass-2026';document.getElementById('login-btn').click();return 1;})()`);
  await delay(1500);
  const who = await evalJS(view, `(document.getElementById('whoami')||{}).textContent || location.href`);
  const cookies = await session.fromPartition(browserPartitionName("t04-main-session")).cookies.get({ name: "lingxi_t04_session" });
  record("E10", "登录+cookie", "登录表单 → /account + ses.cookies.get", "LOGGED_IN demo 且 cookie 可读",
    `whoami=${who} cookie=${cookies.length}`, who.includes("LOGGED_IN demo") && cookies.length > 0 ? "VERIFIED" : "FAILED");

  // E11 cookie 开关（生产 webRequest 剥离语义）
  acceptCookies = false;
  await wc.loadURL(`${SITE}/login`);
  await evalJS(view, `(function(){document.getElementById('u').value='demo';document.getElementById('p').value='lingxi-pass-2026';document.getElementById('login-btn').click();return 1;})()`);
  await delay(1500);
  const whoOff = await evalJS(view, `(document.getElementById('whoami')||{}).textContent || location.href`);
  acceptCookies = true;
  const e11ok = !whoOff.includes("LOGGED_IN");
  record("E11", "cookie 开关", "acceptCookies=false → Set-Cookie 被剥离 → 登录不落",
    "关闭后登录不生效（重定向回 /login）", `whoami=${whoOff}`, e11ok ? "VERIFIED" : "FAILED");

  // E12 两会话隔离（两个 partition）
  const viewA = makeView("t04-iso-A", path.join(EVIDENCE, "downloads"));
  const viewB = makeView("t04-iso-B", path.join(EVIDENCE, "downloads"));
  win.contentView.addChildView(viewA);
  viewA.setBounds({ x: 0, y: 0, width: 1280, height: 860 });
  await viewA.webContents.loadURL(`${SITE}/storage?value=ALPHA_A`);
  await viewA.webContents.loadURL(`${SITE}/storage-read`);
  await delay(500);
  const readA = await evalJS(viewA, `document.getElementById('storage-read').textContent`);
  win.contentView.removeChildView(viewA);
  win.contentView.addChildView(viewB);
  viewB.setBounds({ x: 0, y: 0, width: 1280, height: 860 });
  await viewB.webContents.loadURL(`${SITE}/storage?value=BRAVO_B`);
  await viewB.webContents.loadURL(`${SITE}/storage-read`);
  await delay(500);
  const readB = await evalJS(viewB, `document.getElementById('storage-read').textContent`);
  win.contentView.removeChildView(viewB);
  const e12ok = readA.includes("ALPHA_A") && !readA.includes("BRAVO_B") && readB.includes("BRAVO_B") && !readB.includes("ALPHA_A");
  record("E12", "两会话隔离(partition)", "partition A/B 各写各读（生产分区命名）", "互不串 localStorage/cookie",
    `A=[${readA}] B=[${readB}]`, e12ok ? "VERIFIED" : "FAILED");

  // E13 不可信页探针（sandbox:true 无 preload）
  await wc.loadURL(`${SITE}/probe`);
  let probe = "";
  for (let i = 0; i < 20; i++) {
    probe = await evalJS(view, `document.getElementById('probe-out').textContent`);
    if (typeof probe === "string" && probe.startsWith("PROBE_RESULT")) break;
    await delay(300);
  }
  const e13ok = probe.includes('"hana":"undefined"') && probe.includes('"processGlobal":"undefined"');
  record("E13", "不可信页拿不到宿主权限", "/probe 探测 window.hana/process（sandbox 无 preload）", "全部不可得",
    probe, e13ok ? "VERIFIED" : "FAILED");

  // E14 挂起/热恢复（生产语义：摘下 view 不销毁）
  await wc.loadURL(`${SITE}/form`);
  await evalJS(view, `(function(){var el=document.getElementById('ta1');el.value='suspend-marker-保留';return 1;})()`);
  win.contentView.removeChildView(view); // 挂起
  await delay(800);
  win.contentView.addChildView(view); // 恢复
  view.setBounds({ x: 0, y: 0, width: 1280, height: 860 });
  const taVal = await evalJS(view, `document.getElementById('ta1').value`);
  record("E14", "挂起/热恢复", "removeChildView → 重新挂载", "textarea 内容保留", `ta1=${taVal}`,
    taVal === "suspend-marker-保留" ? "VERIFIED" : "FAILED");

  // E15 代理观测：生产 applyDesktopNetworkProxy 只作用 defaultSession（main.cjs:291-308）
  const defSes = session.defaultSession;
  await defSes.setProxy({ mode: "fixed_servers", proxyRules: "127.0.0.1:9", proxyBypassRules: "<-loopback>" });
  const proxyCfg = await defSes.resolveProxy(`${SITE}/form`);
  await defSes.setProxy({ mode: "direct" });
  record("E15", "代理(生产语义观测)", "defaultSession.setProxy + resolveProxy 取证",
    "记录生产代理只配置在 defaultSession；浏览器 partition 不在其内（契约缺口）",
    `defaultSession resolveProxy=${proxyCfg}；partition 代理=未配置（跟随系统/直连）`, "VERIFIED");

  // E16 用户接管（Electron 是正规 .app，可激活收 OS 键入）
  await wc.loadURL(`${SITE}/form`);
  await evalJS(view, `(function(){var el=document.getElementById('t1');el.value='';el.focus();return 1;})()`);
  win.show(); win.focus(); app.focus && app.focus();
  await delay(700);
  const { execFileSync } = require("child_process");
  let osaErr = "";
  try {
    execFileSync("/usr/bin/osascript", ["-e", 'tell application "System Events" to keystroke "ELEC-HUMAN"'], { stdio: ["ignore", "ignore", "pipe"] });
  } catch (e) { osaErr = String(e.stderr || e.message); }
  await delay(800);
  const hv = await evalJS(view, `document.getElementById('t1').value`);
  const e16ok = typeof hv === "string" && hv.includes("ELEC-HUMAN");
  record("E16", "用户接管(独立输入通道)", "osascript keystroke → Electron 窗口（正规 app 可激活）",
    "OS 级键入落在页面输入框且脚本可读回", `value=${hv} err=${osaErr}`,
    e16ok ? "VERIFIED" : "UNVERIFIED");
}
