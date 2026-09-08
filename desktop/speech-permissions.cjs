/**
 * speech-permissions.cjs — 宿主（Electron 主进程）侧 Speech 框架授权状态机。
 *
 * 职责边界：
 * - 只读查询（getStatus）绝不触发系统授权弹窗；
 * - 授权请求（requestAuthorization）由渲染层用户手势发起，经 IPC 到达这里；
 *   已终结状态（authorized/denied/restricted）直接回包，不再调用原生层
 *   （不反复弹窗、不伪造授权）；
 * - 并发请求单飞合并：同一时刻最多一个在途原生授权请求；
 * - dispose 后在途请求立即按 bridge_unavailable 结算一次，晚到的原生回调丢弃。
 *
 * 原生桥（lingxi-speech-permissions.node）只在 macOS 且首次使用时惰性加载；
 * 桥缺失/加载失败一律报告 bridge_unavailable，不崩不弹窗（fail-closed）。
 */

const path = require("path");
const fs = require("fs");

const SPEECH_PERMISSION_STATES = [
  "not_determined",
  "authorized",
  "denied",
  "restricted",
  "unsupported",
  "bridge_unavailable",
];

// 与 scripts/build-speech-permissions.mjs 的 SPEECH_PERMISSIONS_ARTIFACT_NAME 保持一致。
const SPEECH_PERMISSIONS_ARTIFACT_NAME = "lingxi-speech-permissions.node";
const BRIDGE_ENV_OVERRIDE = "LINGXI_SPEECH_PERMISSIONS_BRIDGE";

// SFSpeechRecognizerAuthorizationStatus rawValue。
const RAW_STATE_MAP = new Map([
  [0, "not_determined"],
  [1, "denied"],
  [2, "restricted"],
  [3, "authorized"],
]);

function stateFromRaw(raw) {
  return RAW_STATE_MAP.get(raw) || "bridge_unavailable";
}

function createSpeechPermissionController({ platform, loadNativeBridge, hostCanPrompt = true } = {}) {
  const effectivePlatform = platform || process.platform;
  const loadBridge = typeof loadNativeBridge === "function"
    ? loadNativeBridge
    : () => {
        throw new Error("no native bridge loader configured");
      };
  // 宿主能否弹真实 TCC 授权窗：仅打包壳（Info.plist 带 NSSpeechRecognitionUsageDescription）
  // 可以。开发壳（node_modules Electron.app）缺该声明——macOS TCC 会在触发授权请求时
  // 直接 SIGABRT 杀掉整个应用（2026-09-08 实测两份崩溃报告）。开发环境一律不请求，
  // 返回 not_determined 由前端走「引导去系统设置」文案。
  const allowHostPrompt = hostCanPrompt !== false;

  let disposed = false;
  let bridge = null;
  let bridgeLoadFailed = false;
  let pendingRequest = null;

  function ensureBridge() {
    if (disposed || bridgeLoadFailed) return null;
    if (bridge) return bridge;
    try {
      const loaded = loadBridge();
      if (!loaded
        || typeof loaded.getAuthorizationStatus !== "function"
        || typeof loaded.requestAuthorization !== "function") {
        throw new Error("native bridge missing required exports");
      }
      bridge = loaded;
      return bridge;
    } catch (err) {
      bridgeLoadFailed = true;
      console.warn(`[speech-permissions] native bridge unavailable: ${err?.message || err}`);
      return null;
    }
  }

  function readCurrentStatus() {
    if (disposed) return "bridge_unavailable";
    if (effectivePlatform !== "darwin") return "unsupported";
    const native = ensureBridge();
    if (!native) return "bridge_unavailable";
    try {
      return stateFromRaw(native.getAuthorizationStatus());
    } catch (err) {
      console.warn(`[speech-permissions] getAuthorizationStatus failed: ${err?.message || err}`);
      return "bridge_unavailable";
    }
  }

  async function getStatus() {
    return readCurrentStatus();
  }

  function startNativeRequest(native) {
    let settleFn = null;
    const request = new Promise((resolve) => {
      let settled = false;
      settleFn = (value) => {
        if (settled) return;
        settled = true;
        if (pendingRequest === request) pendingRequest = null;
        resolve(value);
      };
      try {
        native.requestAuthorization((raw) => {
          // 晚于 dispose 的原生回调：丢弃，不二次结算。
          if (disposed) return;
          settleFn(stateFromRaw(raw));
        });
      } catch (err) {
        console.warn(`[speech-permissions] requestAuthorization failed: ${err?.message || err}`);
        // 同步抛出时 pendingRequest 尚未赋值，延后一拍再结算以便调用方完成登记。
        queueMicrotask(() => settleFn("bridge_unavailable"));
      }
    });
    request.settleUnavailable = () => settleFn("bridge_unavailable");
    return request;
  }

  async function requestAuthorization() {
    if (disposed) return "bridge_unavailable";
    if (effectivePlatform !== "darwin") return "unsupported";
    const native = ensureBridge();
    if (!native) return "bridge_unavailable";
    // 单飞：已有一个在途原生授权请求时，全部等待者加入同一请求。
    if (pendingRequest) return pendingRequest;

    const current = readCurrentStatus();
    if (current !== "not_determined") {
      // authorized/denied/restricted/bridge_unavailable：已终结或不可用，
      // 不再调用原生授权请求（不反复弹窗、不伪造授权）。
      return current;
    }
    if (!allowHostPrompt) {
      // 开发壳不允许触发 TCC 请求（会因缺 usage description 被系统杀进程）。
      // 如实返回 not_determined，前端据此给出设置引导，不伪造授权/拒绝。
      return "not_determined";
    }

    pendingRequest = startNativeRequest(native);
    return pendingRequest;
  }

  function dispose() {
    if (disposed) return;
    disposed = true;
    const pending = pendingRequest;
    pendingRequest = null;
    if (pending && typeof pending.settleUnavailable === "function") {
      // 宿主清理（窗口关闭/app 退出）：在途请求立即按 bridge_unavailable 结算一次。
      pending.settleUnavailable();
    }
    try {
      bridge?.dispose?.();
    } catch {
      // 原生侧清理失败不影响宿主退出。
    }
    bridge = null;
  }

  return { getStatus, requestAuthorization, dispose };
}

/**
 * 默认原生桥加载（生产路径）：override 环境变量 → 打包 Resources → dev dist-speech。
 * 与 helper 的桌面路径合同同源，绝不从 process.execPath 假设推导。
 */
function defaultLoadNativeBridge() {
  const override = process.env[BRIDGE_ENV_OVERRIDE];
  if (override) {
    if (!fs.existsSync(override)) {
      throw new Error(`${BRIDGE_ENV_OVERRIDE} does not exist: ${override}`);
    }
    return require(override);
  }

  let isPackaged = false;
  let resourcesPath = null;
  try {
    const { app } = require("electron");
    isPackaged = Boolean(app?.isPackaged);
    resourcesPath = process.resourcesPath || null;
  } catch {
    isPackaged = false;
  }

  if (isPackaged && resourcesPath) {
    const candidate = path.join(resourcesPath, "speech", "macos", SPEECH_PERMISSIONS_ARTIFACT_NAME);
    if (!fs.existsSync(candidate)) {
      throw new Error(`packaged install corrupt: ${SPEECH_PERMISSIONS_ARTIFACT_NAME} missing at ${candidate}`);
    }
    return require(candidate);
  }

  const arch = process.arch === "x64" ? "mac-x64" : "mac-arm64";
  const devCandidate = path.join(__dirname, "..", "dist-speech", arch, SPEECH_PERMISSIONS_ARTIFACT_NAME);
  if (!fs.existsSync(devCandidate)) {
    throw new Error(`speech permissions bridge not built; run npm run build:speech-permissions (${devCandidate})`);
  }
  return require(devCandidate);
}

let sharedController = null;

function getSharedSpeechPermissionController({ hostCanPrompt } = {}) {
  if (!sharedController) {
    sharedController = createSpeechPermissionController({
      platform: process.platform,
      loadNativeBridge: defaultLoadNativeBridge,
      hostCanPrompt,
    });
  }
  return sharedController;
}

function disposeSharedSpeechPermissionController() {
  sharedController?.dispose?.();
  sharedController = null;
}

module.exports = {
  createSpeechPermissionController,
  getSharedSpeechPermissionController,
  disposeSharedSpeechPermissionController,
  SPEECH_PERMISSION_STATES,
  SPEECH_PERMISSIONS_ARTIFACT_NAME,
};
