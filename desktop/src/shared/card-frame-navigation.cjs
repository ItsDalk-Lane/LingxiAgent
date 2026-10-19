// 互动卡片允许执行脚本，但不能借框架导航向其他地址发送卡片内容或能力票据。
function isCardDocument(value) {
  try {
    const url = new URL(value);
    return /^https?:$/.test(url.protocol) && (/^\/lingxi\/v1\/cards\/[A-Za-z0-9_-]{1,128}\/frame\/[a-f0-9]{64}$/.test(url.pathname) || /^\/api\/cards\/[A-Za-z0-9_-]{1,128}$/.test(url.pathname));
  } catch { return false; }
}
function cardAncestor(frame) {
  for (let current = frame; current; current = current.parent) if (isCardDocument(current.url)) return current;
  return null;
}
function installCardFrameNavigationGuard(contents, onBlocked = () => {}) {
  const guard = event => {
    const current = cardAncestor(event.frame);
    const initiator = cardAncestor(event.initiator);
    if (!current && !initiator) return;
    // 同一文档仅允许原地址重载；宿主设置新票据由可信父页面发起，不属于卡片导航。
    let same = false;
    try { const from = new URL((current || initiator).url); const to = new URL(event.url); from.hash = ''; to.hash = ''; same = from.href === to.href; } catch {}
    const trustedParent = !initiator && event.initiator && isCardDocument(event.url);
    if ((same && current && event.frame === current && !event.isMainFrame) || trustedParent) return;
    event.preventDefault(); onBlocked({ reason:'interactive_card_navigation_blocked' });
  };
  contents.on('will-frame-navigate', guard);
  return () => contents.removeListener('will-frame-navigate', guard);
}
module.exports = { isCardDocument, installCardFrameNavigationGuard };
