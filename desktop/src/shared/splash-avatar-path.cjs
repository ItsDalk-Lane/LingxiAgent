// 启动画面在服务启动前读取同一LINGXI_HOME；只读取既有头像，不设置或更新内容。
const fs = require('fs');
const path = require('path');
const { fileURLToPath } = require('url');
const EXTS = ['png', 'jpg', 'jpeg', 'webp'];
const MAX_BYTES = 15 * 1024 * 1024;

function validAgentId(id) {
  return typeof id === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(id);
}
function checkedPath(home, segments, file = false) {
  const root = path.resolve(home);
  let current = root;
  for (let index = -1; index < segments.length; index++) {
    if (index >= 0) current = path.join(current, segments[index]);
    let stat;
    try { stat = fs.lstatSync(current); } catch (error) {
      if (error.code === 'ENOENT' || error.code === 'ENOTDIR') return null;
      throw error;
    }
    if (stat.isSymbolicLink() || (index === segments.length - 1 && file ? !stat.isFile() : !stat.isDirectory())) return null;
  }
  const canonicalRoot = fs.realpathSync(root);
  return fs.realpathSync(current) === path.join(canonicalRoot, ...segments) ? current : null;
}
function readChecked(home, segments, maxBytes) {
  const candidate = checkedPath(home, segments, true);
  if (!candidate) return null;
  let fd;
  try {
    fd = fs.openSync(candidate, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0) | (fs.constants.O_NONBLOCK || 0));
    const before = fs.fstatSync(fd);
    if (!before.isFile() || before.size < 1 || before.size > maxBytes) return null;
    const bytes = fs.readFileSync(fd);
    const after = fs.fstatSync(fd);
    if (after.size !== before.size || after.mtimeMs !== before.mtimeMs || bytes.length !== before.size) return null;
    if (checkedPath(home, segments, true) !== candidate) return null;
    const visible = fs.lstatSync(candidate);
    if (visible.dev !== before.dev || visible.ino !== before.ino || visible.size !== before.size || visible.mtimeMs !== before.mtimeMs) return null;
    return { path: candidate, bytes };
  } catch (error) {
    if (['ENOENT', 'ENOTDIR', 'ELOOP'].includes(error.code)) return null;
    throw error;
  } finally { if (fd !== undefined) fs.closeSync(fd); }
}
function resolveCurrentSplashAgentId(home, runtime = process.env.LINGXI_DESKTOP_SERVER_RUNTIME || 'node') {
  const valid = id => {
    if (!validAgentId(id) || !checkedPath(home, ['agents', id, 'config.yaml'], true)) return false;
    try { fs.lstatSync(path.join(home, 'agents', id, '.deleted-agent.json')); return false; }
    catch (error) { if (error.code === 'ENOENT') return true; throw error; }
  };
  const preferences = readChecked(home, ['user', 'preferences.json'], 256 * 1024);
  if (preferences) {
    let prefs;
    try { prefs = JSON.parse(preferences.bytes.toString('utf8')); } catch { prefs = null; }
    // Rust现役选择记录使用下划线字段；只在Node模式读取旧壳字段。
    const selected = runtime === 'rust' ? [prefs?.current_agent_id, prefs?.primary_agent_id] : [prefs?.primaryAgent];
    for (const id of selected) if (valid(id)) return id;
  }
  const agents = checkedPath(home, ['agents']);
  if (!agents) return null;
  for (const entry of fs.readdirSync(agents, { withFileTypes: true })) {
    if (entry.isDirectory() && valid(entry.name)) return entry.name;
  }
  return null;
}
function matchesFormat(bytes, ext) {
  if (ext === 'png') return bytes.subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex'));
  if (ext === 'jpg' || ext === 'jpeg') return bytes.length >= 3 && bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255;
  return bytes.length >= 12 && bytes.toString('ascii', 0, 4) === 'RIFF' && bytes.toString('ascii', 8, 12) === 'WEBP';
}
async function resolveSplashAvatarPath(home, role, nativeImage, runtime = process.env.LINGXI_DESKTOP_SERVER_RUNTIME || 'node', decodeWebp) {
  if (role !== 'agent' && role !== 'user') return null;
  const id = role === 'agent' ? resolveCurrentSplashAgentId(home, runtime) : null;
  if (role === 'agent' && !id) return null;
  const base = role === 'user' ? ['user'] : ['agents', id];
  for (const ext of EXTS) {
    const found = readChecked(home, [...base, 'avatars', `${role}.${ext}`], MAX_BYTES);
    if (!found || !matchesFormat(found.bytes, ext)) continue;
    // 检查真实解码，不能把只有图片后缀或伪造文件头的内容交给窗口。
    if (ext === 'webp') {
      if (!decodeWebp || !await decodeWebp(found.bytes)) continue;
      const current = readChecked(home, [...base, 'avatars', `${role}.${ext}`], MAX_BYTES);
      if (!current || !current.bytes.equals(found.bytes)) continue;
    } else if (nativeImage.createFromBuffer(found.bytes).isEmpty()) continue;
    return found.path;
  }
  return null;
}
function isTrustedSplashAvatarSender(event, windows, filePages, devUrl) {
  if (!windows.some(win => win && !win.isDestroyed() && win.webContents === event.sender)) return false;
  if (!event.senderFrame || event.senderFrame !== event.sender.mainFrame) return false;
  let url;
  try { url = new URL(event.senderFrame.url); } catch { return false; }
  url.search = ''; url.hash = '';
  if (url.protocol === 'file:') {
    try { return filePages.some(page => path.resolve(page) === path.resolve(fileURLToPath(url))); } catch { return false; }
  }
  if (!devUrl) return false;
  let dev;
  try { dev = new URL(devUrl); } catch { return false; }
  if (!['http:', 'https:'].includes(dev.protocol) || !['localhost', '127.0.0.1', '[::1]'].includes(dev.hostname)) return false;
  return ['splash', 'onboarding'].some(page => url.href === `${devUrl.replace(/\/$/, '')}/${page}.html`);
}
function readSplashInfo(home, runtime = process.env.LINGXI_DESKTOP_SERVER_RUNTIME || 'node') {
  const fallback = { agentName: null, locale: 'zh-CN', yuan: 'lingxi' };
  const id = resolveCurrentSplashAgentId(home, runtime);
  if (!id) return fallback;
  const config = readChecked(home, ['agents', id, 'config.yaml'], 1024 * 1024);
  if (!config) return fallback;
  let document;
  try { document = require('yaml').parse(config.bytes.toString('utf8')); }
  catch { throw new Error('启动资料格式无效，使用默认信息'); }
  if (!document || typeof document !== 'object' || Array.isArray(document) ||
      (document.agent != null && (typeof document.agent !== 'object' || Array.isArray(document.agent)))) {
    throw new Error('启动资料类型无效，使用默认信息');
  }
  for (const value of [document.agent?.name, document.agent?.yuan, document.locale]) {
    if (value != null && typeof value !== 'string') throw new Error('启动资料字段类型无效，使用默认信息');
  }
  return { agentName: document.agent?.name?.trim() || null, locale: document.locale?.trim() || null, yuan: document.agent?.yuan?.trim() || 'lingxi' };
}
function registerSplashDataIpc({ home, senderAllowed }) {
  const { nativeImage } = require('electron');
  const { wrapIpcHandler } = require('../../ipc-wrapper.cjs');
  wrapIpcHandler('get-avatar-path', async (event, role) => {
    if (!senderAllowed(event)) return null;
    const frame = event.senderFrame;
    const origin = frame.url;
    const result = await resolveSplashAvatarPath(home, role, nativeImage, process.env.LINGXI_DESKTOP_SERVER_RUNTIME || 'node', bytes => {
      // nativeImage只支持PNG/JPEG；WebP用已授权窗口的真实解码器检查字节，不加载URL或放宽CSP。
      const base64 = JSON.stringify(bytes.toString('base64'));
      if (!senderAllowed(event) || frame.url !== origin) return false;
      return frame.executeJavaScript(`(async () => {
        if (location.href !== ${JSON.stringify(origin)}) return false;
        try {
          const bytes = Uint8Array.from(atob(${base64}), c => c.charCodeAt(0));
          const image = await createImageBitmap(new Blob([bytes], { type: 'image/webp' }));
          const valid = image.width > 0 && image.height > 0;
          image.close(); return valid;
        } catch { return false; }
      })()`);
    });
    // 异步解码期间若页面已经离开本地入口，不向新页面回传路径。
    return senderAllowed(event) && frame.url === origin ? result : null;
  });
  wrapIpcHandler('get-splash-info', event => senderAllowed(event) ? readSplashInfo(home) : null);
}
module.exports = { resolveCurrentSplashAgentId, resolveSplashAvatarPath, isTrustedSplashAvatarSender, registerSplashDataIpc, readSplashInfo };
