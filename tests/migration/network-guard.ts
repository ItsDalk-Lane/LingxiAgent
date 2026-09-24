/**
 * R00-A09 断网隔离守卫：在本测试文件的 worker 进程内显式阻断外部网络。
 *
 * 覆盖面（诚实声明）：node:net 的 connect / createConnection 导出、node:tls 的
 * connect（及其 createConnection 导出，若当前 Node 版本存在）、node:dns 的
 * lookup（同步回调与 promises 两个面）及 resolve* 族（回调与 promises）。
 * 管道判定：仅 options.path 显式给出、或字符串参数含 "/"（Unix domain socket 路径）
 * 才视为管道；其余字符串一律按主机名检查（T05 R1-F01 修复，R00-T08 落地）。
 * 残留局限（声明精确性）：本守卫在 worker 启动时打补丁，此前已解构捕获的原函数、
 * 以及 dns.Resolver 类实例内部绑定不受影响——它的作用是「若未来有代码路径意外
 * 外连，测试立即失败」，不是证明被测代码没有其他外联渠道。
 */

import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "::1", "::ffff:127.0.0.1", "[::1]"]);

function isExternalHost(host: unknown): boolean {
  return typeof host === "string" && !LOCAL_HOSTS.has(host);
}

function isPipeTarget(target: Record<string, unknown> | undefined, stringArg: string | undefined): boolean {
  if (target && typeof target.path === "string") return true;
  return typeof stringArg === "string" && stringArg.includes("/");
}

export function installExternalNetworkGuard(context: string): void {
  const net = require("node:net");
  const tls = require("node:tls");
  const dns = require("node:dns");

  const refuse = (label: string, target: unknown) => {
    throw new Error(
      `[${context}] external network blocked: ${label} -> ${JSON.stringify(target)}`,
    );
  };

  const guardNetConnect = (...args: unknown[]) => {
    const target = args.find((a) => typeof a === "object" && a !== null) as Record<string, unknown> | undefined;
    const stringArg = args.find((a) => typeof a === "string") as string | undefined;
    const host = target?.host ?? stringArg;
    if (!isPipeTarget(target, stringArg) && isExternalHost(host)) {
      refuse("net.connect", host);
    }
  };

  const originalNetConnect = net.connect.bind(net);
  net.connect = ((...args: unknown[]) => {
    guardNetConnect(...args);
    return originalNetConnect(...args);
  }) as typeof net.connect;

  // net.createConnection 是与 connect 同一底层函数的另一导出；只补 connect 会被
  // http.Agent 等直接使用 createConnection 的路径绕过（T05 R1-F01 ①②）。
  const originalNetCreateConnection = net.createConnection.bind(net);
  net.createConnection = ((...args: unknown[]) => {
    guardNetConnect(...args);
    return originalNetCreateConnection(...args);
  }) as typeof net.createConnection;

  const guardTlsConnect = (...args: unknown[]) => {
    const target = args.find((a) => typeof a === "object" && a !== null) as Record<string, unknown> | undefined;
    const stringArg = args.find((a) => typeof a === "string") as string | undefined;
    const host = target?.host ?? target?.servername ?? stringArg;
    if (!isPipeTarget(target, stringArg) && isExternalHost(host)) {
      refuse("tls.connect", host);
    }
  };

  const originalTlsConnect = tls.connect.bind(tls);
  tls.connect = ((...args: unknown[]) => {
    guardTlsConnect(...args);
    return originalTlsConnect(...args);
  }) as typeof tls.connect;

  // node:tls 在当前 Node 只导出 connect（无 createConnection 别名）；若未来版本
  // 恢复该导出，则同样纳入补丁（T05 R1-F01 ①的 tls 面）。
  const tlsCreateConnection = (tls as unknown as Record<string, unknown>).createConnection;
  if (typeof tlsCreateConnection === "function") {
    const originalTlsCreateConnection = tlsCreateConnection.bind(tls);
    (tls as unknown as Record<string, unknown>).createConnection = (...args: unknown[]) => {
      guardTlsConnect(...args);
      return originalTlsCreateConnection(...args);
    };
  }

  const originalLookup = dns.lookup.bind(dns);
  dns.lookup = ((...args: unknown[]) => {
    const hostname = args[0];
    if (isExternalHost(hostname)) {
      refuse("dns.lookup", hostname);
    }
    return originalLookup(...args);
  }) as typeof dns.lookup;

  // dns.promises.lookup 与 resolve* 族是独立入口（T05 R1-F01 ④）。
  const promises = dns.promises as unknown as Record<string, unknown>;
  const originalPromisesLookup =
    typeof promises.lookup === "function" ? (promises.lookup as (...a: unknown[]) => unknown).bind(promises) : undefined;
  if (originalPromisesLookup) {
    promises.lookup = (...args: unknown[]) => {
      const hostname = args[0];
      if (isExternalHost(hostname)) {
        return Promise.reject(
          new Error(`[${context}] external network blocked: dns.promises.lookup -> ${JSON.stringify(hostname)}`),
        );
      }
      return originalPromisesLookup(...args);
    };
  }

  const patchResolveFamily = (holder: Record<string, unknown>, labelPrefix: string) => {
    for (const name of Object.keys(holder)) {
      if (!name.startsWith("resolve")) continue;
      const fn = holder[name];
      if (typeof fn !== "function") continue;
      const original = (fn as (...a: unknown[]) => unknown).bind(holder);
      holder[name] = (...args: unknown[]) => {
        const hostname = args[0];
        if (isExternalHost(hostname)) {
          refuse(`${labelPrefix}.${name}`, hostname);
        }
        return original(...args);
      };
    }
  };
  patchResolveFamily(dns as unknown as Record<string, unknown>, "dns");
  patchResolveFamily(promises, "dns.promises");
}
