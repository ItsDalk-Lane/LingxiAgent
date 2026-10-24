/**
 * platform.js — React 之前的宿主可视引导（R08-T02 收口）
 *
 * window.platform / web fallback 的安装已迁移到
 * desktop/src/react/host/（DesktopHost：getDesktopHost 惰性安装，
 * Electron 桥 / Web 受限宿主 / 移动端宿主三选一）。
 *
 * 本文件是非模块 <script>，只保留必须先于 React 执行的可视引导：
 * data-platform 属性 + win32 滚动条 idle 行为。
 * 属于 host 内部白名单文件，允许探测 window.hana。
 */
(function () {
  const bridge = window.hana;
  if (!bridge || typeof bridge.getPlatform !== "function") {
    document.documentElement.setAttribute("data-platform", "web");
    return;
  }
  // Electron：沿用既有行为，抢先安装 window.platform 别名（= preload 桥本体），
  // 保证未迁移的 window.platform 读取在 React 启动前即可用；DesktopHost 惰性收编同一对象。
  window.platform = bridge;
  Promise.resolve(bridge.getPlatform()).then((plat) => {
    document.documentElement.setAttribute("data-platform", plat);
    if (plat === "win32") {
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
    // Windows/Linux 窗口控制按钮在 React (App.tsx WindowControls)
  }).catch(() => {});
})();
