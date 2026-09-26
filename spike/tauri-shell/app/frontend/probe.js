// R01-T06 页面侧探针电池：三类窗口（main 可信 / untrusted 打包未授权 / remote loopback 远端源）
// 加载同一脚本，各自尝试同一组敏感/插件命令，把真实结果写到 window.__T06_RESULT
// （供嵌入式 WebDriver 读取），并尽力 fetch 上报证据服务器（loopback）。
// 任何一步失败都是证据本身，绝不改写为成功。
(async () => {
  const label = document.body.dataset.label || 'unknown';
  const evidenceUrl = 'http://127.0.0.1:19274/report';
  const results = {};
  async function attempt(name, fn) {
    try {
      const v = await fn();
      results[name] = { ok: true, value: typeof v === 'string' ? v : JSON.stringify(v) };
    } catch (e) {
      results[name] = { ok: false, error: String((e && e.message) || e) };
    }
  }
  const withTimeout = (p, ms, tag) =>
    Promise.race([p, new Promise((_, rej) => setTimeout(() => rej(new Error(tag)), ms))]);

  const tauri = window.__TAURI__;
  results.tauri_injected = {
    ok: !!tauri,
    value: tauri ? Object.keys(tauri).join(',') : 'window.__TAURI__ missing',
  };
  let invoke = null;
  if (tauri && tauri.core && typeof tauri.core.invoke === 'function') invoke = tauri.core.invoke.bind(tauri.core);
  else if (window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function')
    invoke = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  results.invoke_available = { ok: !!invoke };

  if (invoke) {
    // 自定义命令：A11 负向用例核心。
    await attempt('shell_public_info', () => invoke('shell_public_info'));
    await attempt('shell_sensitive_probe', () => invoke('shell_sensitive_probe'));
    await attempt('shell_secondary_probe', () => invoke('shell_secondary_probe'));
    await attempt('sidecar_status', () => invoke('sidecar_status'));
    await attempt('sidecar_ping', () => invoke('sidecar_ping'));
    // 未授权窗口不得能收尾/重启宿主（main 不在电池里真实调用 finish_e2e，
    // 否则会提前自杀；main 的 finish 由 runner 经 WebDriver 驱动或 45s 兜底）。
    // main 也不在页面电池里开真实文件对话框（原生模态会卡住 JS）；对话框由宿主侧探针覆盖。
    if (label !== 'main') {
      await attempt('finish_e2e', () => invoke('finish_e2e'));
      await attempt('plugin_dialog_open', () => invoke('plugin:dialog|open', { options: {} }));
    }
    // 插件命令 ACL：未授权窗口连查询都不得放行。
    await attempt('plugin_notification_is_permission_granted', () =>
      invoke('plugin:notification|is_permission_granted'));
    await attempt('plugin_clipboard_read_text', () => invoke('plugin:clipboard-manager|read_text'));
    // 任意 shell：任何窗口都不该有这个权限（插件未向任何 capability 授权）。
    await attempt('plugin_shell_execute', () =>
      invoke('plugin:shell|execute', { program: 'echo', args: ['hi'] }));
    await attempt('totally_unknown_command', () => invoke('shell_exec_arbitrary'));
  }

  // 媒体/显示能力（真实调用；失败如实记录；不伪造授权）
  await attempt('media_enumerate_devices', async () => {
    if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices)
      throw new Error('navigator.mediaDevices unavailable');
    const devs = await withTimeout(navigator.mediaDevices.enumerateDevices(), 5000, 'enumerateDevices timeout');
    return devs.map((d) => `${d.kind}:${d.label ? 'labeled' : 'unlabeled'}`).join(';');
  });
  await attempt('mic_getUserMedia', async () => {
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia)
      throw new Error('getUserMedia unavailable');
    const stream = await withTimeout(
      navigator.mediaDevices.getUserMedia({ audio: true }), 5000, 'timeout-possibly-permission-prompt');
    stream.getTracks().forEach((t) => t.stop());
    return 'audio track acquired';
  });
  await attempt('getDisplayMedia', async () => {
    if (!navigator.mediaDevices || !navigator.mediaDevices.getDisplayMedia)
      throw new Error('getDisplayMedia unavailable');
    const stream = await withTimeout(
      navigator.mediaDevices.getDisplayMedia({ video: true }), 5000, 'timeout-possibly-permission-prompt');
    stream.getTracks().forEach((t) => t.stop());
    return 'display track acquired';
  });

  window.__T06_RESULT = { label, href: location.href, ts: Date.now(), results };
  document.title = 'T06-DONE-' + label;
  try {
    await fetch(evidenceUrl, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(window.__T06_RESULT),
    });
    window.__T06_REPORTED = true;
  } catch (e) {
    window.__T06_REPORTED = 'failed: ' + String(e);
  }

  // main 兜底收尾：runner 正常路径用 WebDriver 驱动 finish_e2e；超时自动收尾防挂死。
  if (label === 'main') {
    setTimeout(() => {
      try {
        if (invoke) invoke('finish_e2e');
      } catch {
        // 兜底语义即"尽力而为"：宿主可能已退出，失败不补记（runner 以 WebDriver 结果为准）
      }
    }, 45000);
  }
})();
