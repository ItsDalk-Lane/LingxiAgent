"use strict";

/**
 * GitHub Release 版本检查 —— Settings > About 的"检查更新"主检测源。
 *
 * 为什么是这个而不是 OTA 签名通道：OTA 通道（ota-core.cjs 的 fetchChannelManifest）
 * 依赖 `LINGXI_ARTIFACT_CHANNEL_BASE_URL` 指向一份签名过的 channel manifest，
 * 而那个环境变量在正式构建里从未配置过——结果 checkOnce 每次都抛
 * "signature or schema verification" 错误，残留在 ota-state.json 永不清除，
 * About 页就常年挂着一条"检查更新失败"。GitHub Releases 不需要签名、
 * 不需要环境变量，仓库（ItsDalk-Lane/LingxiAgent）本身就是发布的 single
 * source of truth，开箱即用。
 *
 * 这个模块只做"最新版本是多少、比当前新不新、去哪下载"三件事，纯函数、
 * 无副作用，fetch 实现从外部注入（生产用 globalThis.fetch，测试用 mock），
 * 网络失败一律降级成 `{ status: "error" }`，绝不抛——About 页据此显示
 * 重试按钮，但不会像 OTA 那样把错误落盘永久残留。
 *
 * 仓库 owner/repo 硬编码为 `ItsDalk-Lane/LingxiAgent`，与 package.json 的
 * electron-builder `publish` 配置保持一致——客户端不读环境变量，因为正式
 * 构建里也没有人会去配。
 */

const RELEASES_LATEST_API = "https://api.github.com/repos/ItsDalk-Lane/LingxiAgent/releases/latest";
const REQUEST_TIMEOUT_MS = 12_000;
const MAX_BODY_CHARS = 64 * 1024; // releases/latest 返回体远小于 list，64KB 足够

/**
 * 解析 `vX.Y.Z` 或 `X.Y.Z` 形式的版本号，返回 [major, minor, patch] 或 null。
 * 与 ota-core.cjs:548 的 parseVersionTriplet 同一套规则，不引新依赖。
 */
function parseVersionTriplet(version) {
  const match = /^v?(\d+)\.(\d+)\.(\d+)/.exec(String(version || "").trim());
  if (!match) return null;
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/**
 * 三段式数值比较：a < b 返回 -1，相等 0，a > b 返回 1。任一侧无法解析
 * 返回 null，由调用方决定"比不了"怎么办（这里一律视为"没有新版本"）。
 */
function compareVersions(a, b) {
  const left = parseVersionTriplet(a);
  const right = parseVersionTriplet(b);
  if (!left || !right) return null;
  for (let i = 0; i < 3; i += 1) {
    if (left[i] !== right[i]) return left[i] > right[i] ? 1 : -1;
  }
  return 0;
}

async function fetchReleaseJson(fetchImpl, url) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    const response = await fetchImpl(url, {
      headers: {
        Accept: "application/vnd.github+json",
        "User-Agent": "LingxiAgent-release-check",
      },
      signal: controller.signal,
    });
    if (!response?.ok) {
      return { ok: false, status: response?.status || "unknown" };
    }
    const text = await response.text();
    if (text.length > MAX_BODY_CHARS) {
      return { ok: false, status: "too-large" };
    }
    return { ok: true, body: JSON.parse(text) };
  } finally {
    clearTimeout(timer);
  }
}

/**
 * 查询 GitHub 最新 release 并与当前版本比对。
 *
 * @param {Object} opts
 * @param {string} opts.currentVersion  当前应用版本（app.getVersion()），如 "0.1.2"
 * @param {Function} [opts.fetchImpl]   fetch 实现，默认 globalThis.fetch
 * @returns {Promise<{status: "latest"|"available"|"error", latestVersion?: string, releaseUrl?: string, error?: string}>}
 *   - latest:    最新 release 不比当前新（含"完全相同"和"比当前旧"——后者不应发生，但当作 latest 处理而非报错）
 *   - available: 发现更新版本，latestVersion/releaseUrl 已填好
 *   - error:     网络/解析失败，error 为人可读的简短说明（不泄露内部细节）
 */
async function checkLatestRelease({ currentVersion, fetchImpl } = {}) {
  const fetchFn = fetchImpl || globalThis.fetch;
  if (typeof fetchFn !== "function") {
    return { status: "error", error: "fetch unavailable" };
  }

  let result;
  try {
    result = await fetchReleaseJson(fetchFn, RELEASES_LATEST_API);
  } catch (err) {
    return { status: "error", error: err?.name === "AbortError" ? "timeout" : "network" };
  }

  if (!result.ok) {
    return { status: "error", error: `request failed (${result.status})` };
  }

  const release = result.body;
  const tagName = String(release?.tag_name || "").trim();
  const latestVersion = tagName.replace(/^v/, "");
  const releaseUrl = String(release?.html_url || "").trim() || null;

  // tag 不像版本号 → 当作"没法判断"，降级成 error 而不是 latest，
  // 避免在 release 发布出问题时静默告诉用户"已是最新"。
  if (!parseVersionTriplet(latestVersion)) {
    return { status: "error", error: "unparsable tag" };
  }

  const cmp = compareVersions(latestVersion, currentVersion);
  if (cmp === null) {
    // 当前版本解析不出来（理论上不会，app.getVersion() 一定是 X.Y.Z）——保守当 error。
    return { status: "error", error: "current version unparsable" };
  }

  if (cmp > 0) {
    return { status: "available", latestVersion, releaseUrl };
  }
  return { status: "latest", latestVersion, releaseUrl };
}

module.exports = {
  checkLatestRelease,
  // 导出纯函数供单测直接断言，不走网络
  parseVersionTriplet,
  compareVersions,
  RELEASES_LATEST_API,
};
