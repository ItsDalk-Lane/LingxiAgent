/**
 * platform.js — React 之前的宿主可视引导（R08-T02 收口 / R09-T01 Tauri）
 *
 * window.platform / web fallback 的安装已迁移到
 * desktop/src/react/host/（DesktopHost：getDesktopHost 惰性安装，
 * Electron 桥 / Tauri 宿主 / Web 受限宿主三选一）。
 *
 * 本文件是非模块 <script>，只保留必须先于 React 执行的可视引导：
 * data-platform 属性 + win32 滚动条 idle 行为。
 * 属于 host 内部白名单文件：只探测 window.hana 与 window.__TAURI__。
 */
(function () {
  function applyPlatform(plat) {
    document.documentElement.setAttribute("data-platform", plat);
    if (plat !== "win32") return;
    const scrollIdleTimers = new WeakMap();
    document.addEventListener("scroll", (event) => {
      if (document.documentElement.getAttribute("data-platform") !== "win32") return;
      const target = event.target;
      if (!(target instanceof Element)) return;

      target.classList.add("hana-scroll-active");
      const previousTimer = scrollIdleTimers.get(target);
      if (previousTimer) clearTimeout(previousTimer);
      const nextTimer = setTimeout(() => {
        target.classList.remove("hana-scroll-active");
        scrollIdleTimers.delete(target);
      }, 800);
      scrollIdleTimers.set(target, nextTimer);
    }, { capture: true, passive: true });
  }

  const bridge = window.hana;
  if (!bridge || typeof bridge.getPlatform !== "function") {
    const tauri = window.__TAURI__;
    if (tauri && tauri.core && typeof tauri.core.invoke === "function") {
      // R09-T01：Tauri 宿主。React 启动前先用 UA 给出近似平台，随后经宿主
      // `get_platform` 异步纠正（词表与 Electron process.platform 一致：
      // darwin / win32 / linux），React 与 CSS 不需要第二套分支。
      const ua = String((typeof navigator !== "undefined" && navigator.userAgent) || "");
      applyPlatform(/Mac/i.test(ua) ? "darwin" : /Windows/i.test(ua) ? "win32" : /Linux/i.test(ua) ? "linux" : "web");
      Promise.resolve(tauri.core.invoke("get_platform")).then((plat) => {
        if (typeof plat === "string" && plat) applyPlatform(plat);
      }).catch(() => {});
      return;
    }
    document.documentElement.setAttribute("data-platform", "web");
    return;
  }
  // Electron：沿用既有行为，抢先安装 window.platform 别名（= preload 桥本体），
  // 保证未迁移的 window.platform 读取在 React 启动前即可用；DesktopHost 惰性收编同一对象。
  window.platform = bridge;
  Promise.resolve(bridge.getPlatform()).then((plat) => {
    applyPlatform(plat);
    // Windows/Linux 窗口控制按钮在 React (App.tsx WindowControls)
  }).catch(() => {});
})();
