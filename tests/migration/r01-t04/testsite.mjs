#!/usr/bin/env node
/**
 * r01-t04-testsite.mjs — R01-T04 受控本地测试站（零依赖 Node，仅绑 127.0.0.1）
 *
 * 页面与端点（全部 deterministic，供三个宿主跑同一操作脚本）：
 *   GET  /                 索引页
 *   GET  /form             表单页：text input / textarea / select / checkbox / 计数按钮 / 文件上传
 *   POST /upload           multipart 接收：回显 filename/size/sha256（不持久化到仓库）
 *   GET  /download/hello.txt  Content-Disposition 附件，内容固定
 *   GET  /popup            window.open 触发页；GET /popup-target 弹窗目标页
 *   GET  /dialog           alert()/confirm() 触发页
 *   GET  /long             长页面（240 行）供滚动与整页截图
 *   GET  /login            登录表单；POST /login 校验固定测试账号并 Set-Cookie
 *   GET  /account          依 cookie 显示登录身份（未登录跳 /login）
 *   POST /logout           清 cookie
 *   GET  /storage?value=   写入 localStorage+document.cookie 并回显当前值
 *   GET  /storage-read     只读回显 localStorage/cookie（隔离判定用）
 *   GET  /probe            不可信页探针：尝试 window.hana / CDP 端口 / node 全局，POST /probe-report
 *   GET  /healthz          200 ok
 *
 * 登录账号（测试专用，非真实账号）：demo / lingxi-pass-2026
 * 用法：node r01-t04-testsite.mjs [port]   默认 18281
 */
import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs";

const PORT = Number(process.argv[2] || 18281);
const HOST = "127.0.0.1";
const SESSION_COOKIE = "lingxi_t04_session";
const sessions = new Set(); // 内存 token 集合；token 本身即“已登录”证据

function page(title, body) {
  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>${title}</title></head><body><h1>${title}</h1>${body}</body></html>`;
}

function parseCookies(req) {
  const out = {};
  const raw = req.headers.cookie || "";
  for (const part of raw.split(";")) {
    const i = part.indexOf("=");
    if (i > 0) out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  }
  return out;
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolve(Buffer.concat(chunks)));
    req.on("error", reject);
  });
}

const LONG_ROWS = Array.from({ length: 240 }, (_, i) =>
  `<div class="row" id="row-${i + 1}">第 ${i + 1} 行 · lingxi-t04-long-row-${String(i + 1).padStart(3, "0")}</div>`,
).join("\n");

const FORM_PAGE = page("T04 表单页", `
<form id="main-form" method="post" action="/submit-form">
  <label>文本框 <input id="t1" name="title" type="text" placeholder="输入标题"></label><br>
  <label>多行框 <textarea id="ta1" name="body" rows="3"></textarea></label><br>
  <label>下拉 <select id="sel1" name="choice">
    <option value="a">选项甲</option><option value="b">选项乙</option><option value="c">选项丙</option>
  </select></label><br>
  <label><input id="cb1" type="checkbox" name="agree" value="yes"> 同意条款</label><br>
  <button id="submit1" type="submit">提交表单</button>
</form>
<button id="counter-btn" type="button">计数:<span id="counter">0</span></button>
<div id="echo" data-testid="echo"></div>
<hr>
<form id="upload-form" method="post" action="/upload" enctype="multipart/form-data">
  <label>文件 <input id="file1" name="file" type="file"></label>
  <button id="upload-btn" type="submit">上传</button>
</form>
<div id="upload-result"></div>
<script>
  let n = 0;
  document.getElementById('counter-btn').addEventListener('click', () => {
    n += 1; document.getElementById('counter').textContent = String(n);
  });
  document.getElementById('main-form').addEventListener('submit', (e) => {
    e.preventDefault();
    const fd = new FormData(e.target);
    document.getElementById('echo').textContent =
      'SUBMITTED title=' + (fd.get('title') || '') + ' choice=' + fd.get('choice') + ' agree=' + fd.get('agree');
  });
  document.getElementById('upload-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const res = await fetch('/upload', { method: 'POST', body: new FormData(e.target) });
    document.getElementById('upload-result').textContent = await res.text();
  });
</script>`);

const POPUP_PAGE = page("T04 弹窗页", `
<button id="open-popup" type="button">打开弹窗</button>
<div id="popup-state">not-opened</div>
<script>
  document.getElementById('open-popup').addEventListener('click', () => {
    window.open('/popup-target', 't04popup', 'width=480,height=320');
    document.getElementById('popup-state').textContent = 'opened';
  });
</script>`);

const POPUP_TARGET = page("T04 弹窗目标", `
<div id="popup-marker">POPUP_TARGET_LOADED</div>
<script>
  try { window.opener && window.opener.postMessage('popup-alive', '*'); } catch (e) {}
</script>`);

const DIALOG_PAGE = page("T04 对话框页", `
<button id="alert-btn" type="button">触发alert</button>
<button id="confirm-btn" type="button">触发confirm</button>
<div id="dialog-result">none</div>
<script>
  document.getElementById('alert-btn').addEventListener('click', () => {
    alert('t04-alert-你好');
    document.getElementById('dialog-result').textContent = 'alert-done';
  });
  document.getElementById('confirm-btn').addEventListener('click', () => {
    const ok = confirm('t04-confirm?');
    document.getElementById('dialog-result').textContent = 'confirm:' + ok;
  });
</script>`);

const LONG_PAGE = page("T04 长页面", `<div id="long-list">${LONG_ROWS}</div>
<div id="bottom-marker">BOTTOM_REACHED</div>
<style>.row{height:48px;border-bottom:1px solid #ddd;font-family:monospace}</style>`);

const LOGIN_PAGE = page("T04 登录", `
<form id="login-form" method="post" action="/login">
  <label>账号 <input id="u" name="username" type="text"></label><br>
  <label>密码 <input id="p" name="password" type="password"></label><br>
  <button id="login-btn" type="submit">登录</button>
</form>
<div id="login-error"></div>`);

const PROBE_PAGE = page("T04 不可信探针页", `
<pre id="probe-out">probing…</pre>
<script>
(async () => {
  const out = {};
  // 1. 宿主注入对象探测
  out.hana = typeof window.hana;
  out.hanaKeys = typeof window.hana === 'object' && window.hana ? Object.keys(window.hana).slice(0, 20) : [];
  out.processGlobal = typeof window.process;
  out.requireGlobal = typeof window.require;
  out.nodeIntegration = (typeof window.process === 'object' && window.process && window.process.versions && window.process.versions.node) || null;
  // 2. CDP 调试端口语义探测（页面 JS 主动外联宿主调试面）
  const tryFetch = async (url) => {
    try { const r = await fetch(url, { signal: AbortSignal.timeout(1500) }); return 'HTTP_' + r.status; }
    catch (e) { return 'BLOCKED:' + (e && e.name || 'err'); }
  };
  out.cdpHttpJson = await tryFetch('http://127.0.0.1:9222/json/version');
  out.cdpHttpLocalhost = await tryFetch('http://localhost:9222/json/version');
  out.cdpWs = await new Promise((resolve) => {
    try {
      const ws = new WebSocket('ws://127.0.0.1:9222/devtools/browser');
      const timer = setTimeout(() => { try { ws.close(); } catch (_) {}; resolve('TIMEOUT_CLOSED'); }, 1500);
      ws.onopen = () => { clearTimeout(timer); resolve('OPENED'); };
      ws.onerror = () => { clearTimeout(timer); resolve('BLOCKED:error'); };
    } catch (e) { resolve('BLOCKED:' + (e && e.name || 'err')); }
  });
  // 3. 文件系统/原生能力探测
  out.showOpenFilePicker = typeof window.showOpenFilePicker;
  document.getElementById('probe-out').textContent = 'PROBE_RESULT ' + JSON.stringify(out);
  try { await fetch('/probe-report', { method: 'POST', body: JSON.stringify(out) }); } catch (e) {}
})();
</script>`);

const server = http.createServer(async (req, res) => {
  const u = new URL(req.url, `http://${HOST}:${PORT}`);
  const send = (code, body, headers = {}) => {
    res.writeHead(code, { "content-type": "text/html; charset=utf-8", ...headers });
    res.end(body);
  };
  try {
    if (u.pathname === "/healthz") return send(200, "ok", { "content-type": "text/plain" });
    if (u.pathname === "/") {
      return send(200, page("T04 测试站索引", `
        <ul>
          <li><a href="/form">表单页</a></li>
          <li><a href="/popup">弹窗页</a></li>
          <li><a href="/dialog">对话框页</a></li>
          <li><a href="/long">长页面</a></li>
          <li><a href="/login">登录</a></li>
          <li><a href="/storage">storage 写入页</a></li>
          <li><a href="/storage-read">storage 读取页</a></li>
          <li><a href="/probe">不可信探针页</a></li>
          <li><a href="/download/hello.txt">下载 hello.txt</a></li>
        </ul>`));
    }
    if (u.pathname === "/form") return send(200, FORM_PAGE);
    if (u.pathname === "/popup") return send(200, POPUP_PAGE);
    if (u.pathname === "/popup-target") return send(200, POPUP_TARGET);
    if (u.pathname === "/dialog") return send(200, DIALOG_PAGE);
    if (u.pathname === "/long") return send(200, LONG_PAGE);
    if (u.pathname === "/login" && req.method === "GET") return send(200, LOGIN_PAGE);
    if (u.pathname === "/login" && req.method === "POST") {
      const body = (await readBody(req)).toString("utf-8");
      const params = new URLSearchParams(body);
      if (params.get("username") === "demo" && params.get("password") === "lingxi-pass-2026") {
        const token = crypto.randomBytes(12).toString("hex");
        sessions.add(token);
        // 持久化 cookie（Max-Age=24h）：登录持久化语义的验收对象。
        // 注意：不带 Max-Age/Expires 的 session cookie 按 Chromium 语义不随
        // 进程重启保留（两个宿主一致），持久化测试必须用持久 cookie。
        res.writeHead(302, {
          location: "/account",
          "set-cookie": `${SESSION_COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400`,
        });
        return res.end();
      }
      return send(401, page("登录失败", `<div id="login-error">INVALID_CREDENTIALS</div>`));
    }
    if (u.pathname === "/account") {
      const token = parseCookies(req)[SESSION_COOKIE];
      if (token && sessions.has(token)) {
        return send(200, page("T04 账户页", `<div id="whoami">LOGGED_IN demo</div><div id="token-len">${token.length}</div>`));
      }
      res.writeHead(302, { location: "/login" });
      return res.end();
    }
    if (u.pathname === "/logout" && req.method === "POST") {
      const token = parseCookies(req)[SESSION_COOKIE];
      if (token) sessions.delete(token);
      res.writeHead(302, { location: "/login", "set-cookie": `${SESSION_COOKIE}=; Path=/; Max-Age=0` });
      return res.end();
    }
    if (u.pathname === "/download/hello.txt") {
      const content = "lingxi-t04-download-payload-你好-0123456789\n";
      res.writeHead(200, {
        "content-type": "text/plain; charset=utf-8",
        "content-disposition": 'attachment; filename="hello.txt"',
      });
      return res.end(content);
    }
    if (u.pathname === "/upload" && req.method === "POST") {
      const body = await readBody(req);
      const ct = req.headers["content-type"] || "";
      const m = /boundary=(.+)$/.exec(ct);
      if (!m) return send(400, "NO_BOUNDARY", { "content-type": "text/plain" });
      const boundary = m[1];
      const text = body.toString("latin1");
      const fm = /filename="([^"]*)"/.exec(text);
      const filename = fm ? fm[1] : "";
      // 提取文件内容（两个 CRLF 之后到 boundary 之前）
      let payloadSha = "";
      let payloadSize = 0;
      if (filename) {
        const start = body.indexOf("\r\n\r\n") + 4;
        const endMarker = Buffer.from("\r\n--" + boundary, "latin1");
        const end = body.indexOf(endMarker, start);
        const payload = body.subarray(start, end > 0 ? end : body.length);
        payloadSize = payload.length;
        payloadSha = crypto.createHash("sha256").update(payload).digest("hex");
      }
      return send(200, `UPLOAD_OK filename=${filename} size=${payloadSize} sha256=${payloadSha}`, { "content-type": "text/plain; charset=utf-8" });
    }
    if (u.pathname === "/storage") {
      const value = u.searchParams.get("value") || "default";
      return send(200, page("T04 storage 写入", `
        <div id="storage-state">pending</div>
        <script>
          localStorage.setItem('t04-value', ${JSON.stringify(value)});
          document.cookie = 't04-cookie=' + encodeURIComponent(${JSON.stringify(value)}) + '; path=/; SameSite=Lax';
          document.getElementById('storage-state').textContent =
            'STORAGE_SET local=' + localStorage.getItem('t04-value') + ' cookie=' + document.cookie;
        </script>`));
    }
    if (u.pathname === "/storage-read") {
      return send(200, page("T04 storage 读取", `
        <div id="storage-read">pending</div>
        <script>
          document.getElementById('storage-read').textContent =
            'STORAGE_READ local=' + localStorage.getItem('t04-value') + ' cookie=' + document.cookie;
        </script>`));
    }
    if (u.pathname === "/probe") return send(200, PROBE_PAGE);
    if (u.pathname === "/probe-report" && req.method === "POST") {
      const body = await readBody(req);
      // 记录到 stderr，由驱动脚本采集
      console.error(`[probe-report] ${body.toString("utf-8")}`);
      return send(200, "recorded", { "content-type": "text/plain" });
    }
    return send(404, page("404", `<p>not found: ${u.pathname}</p>`));
  } catch (err) {
    return send(500, `server error: ${err && err.message}`, { "content-type": "text/plain" });
  }
});

server.listen(PORT, HOST, () => {
  console.log(`[r01-t04-testsite] listening http://${HOST}:${PORT}`);
});
