/**
 * R00-A09 断网隔离守卫：在本测试文件的 worker 进程内显式阻断外部网络。
 *
 * 覆盖面（诚实声明）：node:net / node:tls 的 connect、node:dns 的 lookup。
 * 回放本体不发起任何连接；本守卫的作用是「若未来有代码路径意外外连，测试立即失败」，
 * 而不是证明被测代码没有其他外联渠道。
 */

import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "::1", "::ffff:127.0.0.1", "[::1]"]);

export function installExternalNetworkGuard(context: string): void {
  const net = require("node:net");
  const tls = require("node:tls");
  const dns = require("node:dns");

  const refuse = (label: string, target: unknown) => {
    throw new Error(
      `[${context}] external network blocked: ${label} -> ${JSON.stringify(target)}`,
    );
  };

  const originalNetConnect = net.connect.bind(net);
  net.connect = ((...args: unknown[]) => {
    const target = args.find((a) => typeof a === "object" && a !== null) as Record<string, unknown> | undefined;
    const stringArg = args.find((a) => typeof a === "string") as string | undefined;
    const host = (target?.host ?? stringArg) as unknown;
    const isPipe = (typeof target?.path === "string") || (typeof stringArg === "string" && !stringArg.includes(":"));
    if (!isPipe && typeof host === "string" && !LOCAL_HOSTS.has(host)) {
      refuse("net.connect", host);
    }
    return originalNetConnect(...args);
  }) as typeof net.connect;

  const originalTlsConnect = tls.connect.bind(tls);
  tls.connect = ((...args: unknown[]) => {
    const target = args.find((a) => typeof a === "object" && a !== null) as Record<string, unknown> | undefined;
    const stringArg = args.find((a) => typeof a === "string") as string | undefined;
    const host = (target?.host ?? target?.servername ?? stringArg) as unknown;
    if (typeof host === "string" && !LOCAL_HOSTS.has(host)) {
      refuse("tls.connect", host);
    }
    return originalTlsConnect(...args);
  }) as typeof tls.connect;

  const originalLookup = dns.lookup.bind(dns);
  dns.lookup = ((...args: unknown[]) => {
    const hostname = args[0];
    if (typeof hostname === "string" && !LOCAL_HOSTS.has(hostname)) {
      refuse("dns.lookup", hostname);
    }
    return originalLookup(...args);
  }) as typeof dns.lookup;
}
