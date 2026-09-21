import fs from "fs";
import path from "path";
// P01-T07：LINGXI_HOME 解析统一到 shared/hana-runtime-paths 权威实现
// （P00 OWNERSHIP_MAP 🔴 双实现收口）。CLI 侧保留的唯一语义是"从 env 对象取值
// 并 trim"，其余（空值默认、~ 展开、resolve）与 server/desktop 同源。
import { resolveLingxiHome } from "../shared/hana-runtime-paths.ts";

export function resolveCliLingxiHome(env: { LINGXI_HOME?: string | undefined } = process.env) {
  const raw = typeof env.LINGXI_HOME === "string" ? env.LINGXI_HOME.trim() : "";
  return resolveLingxiHome(raw || undefined);
}

type LocalServerInfo = {
  port: number;
  token: string;
  pid?: number;
  version?: string;
};

export function readLocalServerInfo({ lingxiHome = resolveCliLingxiHome(), checkProcess = true } = {}) {
  const filePath = path.join(lingxiHome, "server-info.json");
  if (!fs.existsSync(filePath)) {
    return {
      ok: false as const,
      reason: "missing_server_info" as const,
      filePath,
      message: `No running LingxiAgent Server was found at ${filePath}`,
    };
  }

  let info: LocalServerInfo;
  try {
    info = JSON.parse(fs.readFileSync(filePath, "utf8")) as LocalServerInfo;
  } catch (err) {
    return {
      ok: false as const,
      reason: "invalid_server_info" as const,
      filePath,
      message: `Cannot read ${filePath}: ${err instanceof Error ? err.message : String(err)}`,
    };
  }

  if (!Number.isInteger(info?.port) || (info?.port ?? 0) <= 0 || !info?.token) {
    return {
      ok: false as const,
      reason: "incomplete_server_info" as const,
      filePath,
      message: `${filePath} is missing port or token`,
    };
  }

  const pid = info?.pid;
  if (checkProcess && typeof pid === "number" && Number.isInteger(pid) && !isProcessAlive(pid)) {
    return {
      ok: false as const,
      reason: "stale_server_info" as const,
      filePath,
      message: `LingxiAgent Server process ${info.pid} is no longer running`,
    };
  }

  return {
    ok: true as const,
    filePath,
    info,
    baseUrl: `http://127.0.0.1:${info.port}`,
    token: info.token,
    source: "server-info" as const,
  };
}

export function resolveConnection({ url, token, lingxiHome }: { url?: string; token?: string; lingxiHome?: string } = {}) {
  if (url) {
    return {
      ok: true as const,
      baseUrl: stripTrailingSlash(url),
      token: token || "",
      source: "explicit" as const,
      queryTokenAllowed: false,
    };
  }

  const local = readLocalServerInfo({ lingxiHome });
  if (!local.ok) return local;
  return {
    ...local,
    baseUrl: stripTrailingSlash(local.baseUrl),
    queryTokenAllowed: true,
  };
}

export function isProcessAlive(pid: number) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function stripTrailingSlash(value: string) {
  return String(value || "").replace(/\/+$/, "");
}
