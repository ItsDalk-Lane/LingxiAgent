/**
 * P07-T02｜启动可选能力惰性初始化的并发语义（A03 场景行为测试）。
 *
 * 被测对象：server/index.ts 的 startBridgeManager（生产闭包函数，源码提取 +
 * vm 执行；提取失败=测试失败，防止静默漂移）。该函数是启动链中「真正可后台
 * 延后模块」的代表：server-info.json 写入后 setImmediate 后台加载（结构契约
 * 已由 tests/server-startup-diagnostics-contract.test.ts:491 锁定），首次使用
 * 经 bridgeManagerRef.ensureReady() 触发。
 *
 * 行为验证（A03：惰性模块未加载，两个请求同时到达）：
 *   1. 并发 ensureReady ×2 → 动态 import 恰好一次、两调用拿到同一 manager 实例；
 *   2. 初始化失败 → 错误捕获并可从 getState() 观察（不静默、不虚报就绪），
 *      promise 清空后下一次调用可重试；
 *   3. 成功后的后续调用直接返回缓存实例，不再触发 import（首次使用只初始化一次）。
 *
 * 桥接模块以计数替身注入（真实 BridgeManager 的加载与平台副作用不在本测试范围，
 * 其生命周期由 bridge 系列测试覆盖）。
 */
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const root = process.cwd();

function extractStartBridgeManagerSource(): string {
  const serverSource = fs.readFileSync(path.join(root, "server", "index.ts"), "utf-8");
  const start = serverSource.indexOf("async function startBridgeManager");
  if (start === -1) throw new Error("startBridgeManager 函数未找到（源码漂移，需同步本测试）");
  // 跳过参数列表（参数含解构大括号，不能从第一个 { 起算）：先配平圆括号再取函数体大括号
  const parenStart = serverSource.indexOf("(", start);
  let parenDepth = 0;
  let bodyStart = -1;
  for (let i = parenStart; i < serverSource.length; i++) {
    const ch = serverSource[i];
    if (ch === "(") parenDepth += 1;
    else if (ch === ")") {
      parenDepth -= 1;
      if (parenDepth === 0) {
        bodyStart = serverSource.indexOf("{", i);
        break;
      }
    }
  }
  if (bodyStart === -1) throw new Error("startBridgeManager 参数/函数体定位失败（源码漂移，需同步本测试）");
  // 大括号配平截取函数体
  let depth = 0;
  let end = -1;
  let began = false;
  for (let i = bodyStart; i < serverSource.length; i++) {
    const ch = serverSource[i];
    if (ch === "{") { depth += 1; began = true; }
    else if (ch === "}") {
      depth -= 1;
      if (began && depth === 0) { end = i + 1; break; }
    }
  }
  if (end === -1) throw new Error("startBridgeManager 函数体截取失败");
  const source = serverSource.slice(start, end);
  // 动态 import 替换为可控加载器（保留其余源码逐字执行）
  const importLiteral = 'await import("../lib/bridge/bridge-manager.ts")';
  if (!source.includes(importLiteral)) {
    throw new Error("startBridgeManager 中的动态 import 字面量未找到（源码漂移，需同步本测试）");
  }
  return source.replace(importLiteral, "await __loadBridge()");
}

interface Harness {
  ensureReady: () => Promise<unknown>;
  getState: () => { ready: boolean; initializing: boolean; error: string | null };
  loader: { calls: number; controller: { resolveWith: (v: unknown) => void; rejectWith: (e: Error) => void } | null };
}

function buildHarness(_loaderImpl?: unknown): Harness {
  const fnSource = extractStartBridgeManagerSource();
  const state = {
    loaderCalls: 0,
    pending: null as null | { resolveWith: (v: unknown) => void; rejectWith: (e: Error) => void },
  };
  const script = new vm.Script(`
    let bridgeManager = null;
    let bridgeManagerInitPromise = null;
    let bridgeManagerInitError = null;
    let bridgeAutoStartRequested = false;
    const log = { log() {}, error() {} };
    const dlog = { error() {} };
    const engine = {};
    const hub = {};
    const runBridgeAutoStart = () => {};
    const __loadBridge = () => __loaderImpl();
    ${fnSource}
    // 与生产同款的引用面（server/index.ts startBridgeManager 之后立即构造）
    const bridgeManagerRef = {
      get: () => bridgeManager,
      ensureReady: () => startBridgeManager(),
      getState: () => ({
        ready: !!bridgeManager,
        initializing: !!bridgeManagerInitPromise,
        error: bridgeManagerInitError?.message || null,
      }),
    };
    __ensureReady(bridgeManagerRef.ensureReady);
    __getStateRef(bridgeManagerRef);
  `);
  const context = vm.createContext({
    __loaderImpl: () => {
      state.loaderCalls += 1;
      return new Promise((resolve, reject) => {
        state.pending = { resolveWith: resolve, rejectWith: reject };
      });
    },
    __ensureReady: (fn: () => Promise<unknown>) => { harness.ensureReady = fn; },
    __getStateRef: (ref: { getState: () => { ready: boolean; initializing: boolean; error: string | null } }) => { harness.getState = () => ref.getState(); },
    // async 函数源码在 vm.Script 中以 await 顶层形式出现的是函数体内部，无需顶层 await 支持
    Promise, Error,
  });
  const harness = {} as Harness;
  script.runInContext(context);
  harness.loader = {
    get calls() { return state.loaderCalls; },
    controller: null as never,
  };
  // pending 控制面：每次 loader 被调用时挂上
  const loader = harness.loader as unknown as { calls: number; controller: { resolveWith: (v: unknown) => void; rejectWith: (e: Error) => void } | null };
  Object.defineProperty(loader, "controller", {
    get: () => state.pending,
  });
  return harness;
}

describe("P07-T02 启动可选能力惰性初始化（A03：首次调用并发）", () => {
  it("并发 ensureReady ×2：动态加载恰好一次，两调用拿到同一实例", async () => {
    const harness = buildHarness(async () => { throw new Error("not used"); });
    const first = harness.ensureReady();
    const second = harness.ensureReady();
    expect(harness.loader.calls).toBe(1);
    expect(harness.getState().initializing).toBe(true);
    const managerInstance = { __bridgeManager: true };
    harness.loader.controller!.resolveWith({ BridgeManager: class { constructor() { return managerInstance; } } });
    const [a, b] = await Promise.all([first, second]);
    expect(a).toBe(managerInstance);
    expect(b).toBe(managerInstance);
    expect(harness.getState().ready).toBe(true);
    expect(harness.getState().initializing).toBe(false);
    expect(harness.getState().error).toBeNull();
  });

  it("初始化失败：错误可观察（getState().error），不虚报就绪；下次调用可重试", async () => {
    const harness = buildHarness(async () => { throw new Error("not used"); });
    const first = harness.ensureReady();
    harness.loader.controller!.rejectWith(new Error("bridge dependency load failed"));
    await expect(first).resolves.toBeNull(); // 生产语义：失败收口为 null，不抛出
    expect(harness.getState().ready).toBe(false);
    expect(harness.getState().error).toBe("bridge dependency load failed");
    // 重试：promise 已清空，新的调用再次发起加载
    const retry = harness.ensureReady();
    expect(harness.loader.calls).toBe(2);
    class Retried { toString() { return "retried"; } }
    harness.loader.controller!.resolveWith({ BridgeManager: Retried });
    await expect(retry.then((v) => (v as unknown as Retried).toString())).resolves.toBe("retried");
  });

  it("成功后后续调用直接返回缓存实例，不再触发加载（首次使用只初始化一次）", async () => {
    const harness = buildHarness(async () => { throw new Error("not used"); });
    const first = harness.ensureReady();
    const managerInstance = { __cached: true };
    harness.loader.controller!.resolveWith({ BridgeManager: class { constructor() { return managerInstance; } } });
    await expect(first).resolves.toBe(managerInstance);
    for (let i = 0; i < 5; i++) {
      await expect(harness.ensureReady()).resolves.toBe(managerInstance);
    }
    expect(harness.loader.calls).toBe(1);
  });
});
