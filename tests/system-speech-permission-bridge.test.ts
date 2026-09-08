/**
 * A01–A05 / A15：宿主 Speech 授权桥（desktop/speech-permissions.cjs）状态机。
 *
 * 原生 .node 桥在此以注入替身替代——本文件验证的是授权状态机与单飞/单次结算
 * 语义；真实原生桥的真实权限弹窗验收（A02/A16 的真机部分）单独标记 BLOCKED。
 */
import { createRequire } from "module";
import { describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);
const {
  createSpeechPermissionController,
  SPEECH_PERMISSION_STATES,
} = require("../desktop/speech-permissions.cjs");

// SFSpeechRecognizerAuthorizationStatus rawValue：0 notDetermined / 1 denied / 2 restricted / 3 authorized
function makeNativeBridge(initialRaw = 0) {
  const state = { raw: initialRaw, callbacks: [] };
  const native = {
    getAuthorizationStatus: vi.fn(() => state.raw),
    requestAuthorization: vi.fn((cb) => {
      state.callbacks.push(cb);
    }),
    dispose: vi.fn(() => {
      state.callbacks = [];
    }),
    // 测试驱动：模拟原生授权回调到达。
    __resolve(stateRaw) {
      state.raw = stateRaw;
      const pending = [...state.callbacks];
      state.callbacks = [];
      for (const cb of pending) cb(stateRaw);
    },
    __pendingCount: () => state.callbacks.length,
  };
  return native;
}

describe("system speech permission bridge（A01–A05, A15）", () => {
  it("A01：status 查询只读不弹权限", async () => {
    const native = makeNativeBridge(0);
    const controller = createSpeechPermissionController({
      platform: "darwin",
      loadNativeBridge: () => native,
    });
    await expect(controller.getStatus()).resolves.toBe("not_determined");
    await expect(controller.getStatus()).resolves.toBe("not_determined");
    expect(native.getAuthorizationStatus).toHaveBeenCalled();
    // 状态查询绝不触发授权请求（不弹窗）。
    expect(native.requestAuthorization).not.toHaveBeenCalled();
  });

  it("A02：首次用户触发授权——同意与拒绝都回包准确", async () => {
    const granting = makeNativeBridge(0);
    const controller = createSpeechPermissionController({
      platform: "darwin",
      loadNativeBridge: () => granting,
    });
    const pending = controller.requestAuthorization();
    // 授权请求确实到达原生层一次。
    expect(granting.requestAuthorization).toHaveBeenCalledTimes(1);
    granting.__resolve(3);
    await expect(pending).resolves.toBe("authorized");
    await expect(controller.getStatus()).resolves.toBe("authorized");

    const denying = makeNativeBridge(0);
    const deniedController = createSpeechPermissionController({
      platform: "darwin",
      loadNativeBridge: () => denying,
    });
    const deniedPending = deniedController.requestAuthorization();
    denying.__resolve(1);
    await expect(deniedPending).resolves.toBe("denied");
    await expect(deniedController.getStatus()).resolves.toBe("denied");
  });

  it("A03：并发请求合并为一个在途原生授权，全部等待者同结果", async () => {
    const native = makeNativeBridge(0);
    const controller = createSpeechPermissionController({
      platform: "darwin",
      loadNativeBridge: () => native,
    });
    const first = controller.requestAuthorization();
    const second = controller.requestAuthorization();
    const third = controller.requestAuthorization();
    // 单飞：三个并发调用只产生一个原生授权请求（不连续弹窗）。
    expect(native.requestAuthorization).toHaveBeenCalledTimes(1);
    native.__resolve(3);
    await expect(first).resolves.toBe("authorized");
    await expect(second).resolves.toBe("authorized");
    await expect(third).resolves.toBe("authorized");
  });

  it("A04：已拒绝/restricted 不循环请求、不伪造授权", async () => {
    for (const [raw, expected] of [[1, "denied"], [2, "restricted"]] as Array<[number, string]>) {
      const native = makeNativeBridge(raw);
      const controller = createSpeechPermissionController({
        platform: "darwin",
        loadNativeBridge: () => native,
      });
      await expect(controller.getStatus()).resolves.toBe(expected);
      // 已终结的否定状态：请求授权不再调用原生层（不反复弹窗企图绕过）。
      await expect(controller.requestAuthorization()).resolves.toBe(expected);
      expect(native.requestAuthorization).not.toHaveBeenCalled();
    }
  });

  it("A05：原生回调晚于 dispose——无越界、无重复结算", async () => {
    const native = makeNativeBridge(0);
    const controller = createSpeechPermissionController({
      platform: "darwin",
      loadNativeBridge: () => native,
    });
    let settleCount = 0;
    const pending = controller.requestAuthorization().then((value) => {
      settleCount += 1;
      return value;
    });
    expect(native.requestAuthorization).toHaveBeenCalledTimes(1);
    // 宿主清理（窗口关闭/app 退出）：在途请求立即按 bridge_unavailable 结算一次。
    controller.dispose();
    await expect(pending).resolves.toBe("bridge_unavailable");
    expect(settleCount).toBe(1);
    // 晚到的原生回调：不得抛错、不得二次结算。
    expect(() => native.__resolve(3)).not.toThrow();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(settleCount).toBe(1);
    // dispose 之后的查询不触碰已销毁的原生环境。
    await expect(controller.getStatus()).resolves.toBe("bridge_unavailable");
    expect(native.getAuthorizationStatus.mock.calls.length).toBeLessThanOrEqual(1);
  });

  it("A15：非 macOS 返回 unsupported，且不加载 macOS 原生模块", async () => {
    for (const platform of ["win32", "linux"]) {
      const loadNativeBridge = vi.fn(() => {
        throw new Error("must not be called on non-darwin");
      });
      const controller = createSpeechPermissionController({ platform, loadNativeBridge });
      await expect(controller.getStatus()).resolves.toBe("unsupported");
      await expect(controller.requestAuthorization()).resolves.toBe("unsupported");
      expect(loadNativeBridge).not.toHaveBeenCalled();
    }
  });

  it("A01b：原生桥缺失（未构建）时报告 bridge_unavailable，不崩不弹窗", async () => {
    const controller = createSpeechPermissionController({
      platform: "darwin",
      loadNativeBridge: () => {
        throw new Error("Cannot find module 'lingxi-speech-permissions.node'");
      },
    });
    await expect(controller.getStatus()).resolves.toBe("bridge_unavailable");
    await expect(controller.requestAuthorization()).resolves.toBe("bridge_unavailable");
  });

  it("hostCanPrompt=false（开发壳）：not_determined 时不触发原生授权请求，原样返回 not_determined", async () => {
    // 背景：开发壳（node_modules Electron.app）Info.plist 缺 NSSpeechRecognitionUsageDescription，
    // 触发 TCC 请求会被系统 SIGABRT 杀掉整个应用（2026-09-08 实测）。开发环境必须走引导而非弹窗。
    let requestCalls = 0;
    const controller = createSpeechPermissionController({
      platform: "darwin",
      hostCanPrompt: false,
      loadNativeBridge: () => ({
        getAuthorizationStatus: () => 0,
        requestAuthorization: (_cb: (raw: number) => void) => { requestCalls += 1; },
        dispose: () => {},
      }),
    });
    await expect(controller.requestAuthorization()).resolves.toBe("not_determined");
    expect(requestCalls).toBe(0);
    // 默认（打包壳，hostCanPrompt 未传）仍然走原生请求
    const callbacks: Array<(raw: number) => void> = [];
    const prompting = createSpeechPermissionController({
      platform: "darwin",
      loadNativeBridge: () => ({
        getAuthorizationStatus: () => 0,
        requestAuthorization: (cb: (raw: number) => void) => { callbacks.push(cb); },
        dispose: () => {},
      }),
    });
    const pending = prompting.requestAuthorization();
    expect(callbacks).toHaveLength(1);
    callbacks[0](3);
    await expect(pending).resolves.toBe("authorized");
  });

  it("状态集闭合约定型", () => {
    expect(SPEECH_PERMISSION_STATES).toEqual([
      "not_determined",
      "authorized",
      "denied",
      "restricted",
      "unsupported",
      "bridge_unavailable",
    ]);
  });
});
