/**
 * Settings > About update history.
 *
 * GitHub Releases is the release source of truth. The installed v2 anthology is
 * only an explicit offline fallback because older app packages cannot contain
 * releases published after they were built.
 *
 * The release source is fully configuration-driven: there is no built-in
 * default repository. Configure it via the loader options (`releasesApiUrl` /
 * `releaseAssetBaseUrl`) or the environment variables
 * LINGXI_UPDATE_RELEASES_API_URL / LINGXI_UPDATE_DIGEST_BASE_URL /
 * LINGXI_UPDATE_GITHUB_OWNER / LINGXI_UPDATE_GITHUB_REPO. With no source
 * configured the online load is skipped and the bundled anthology is used.
 */

const DIGEST_ASSET_NAME = "release-digest.v1.json";
const HISTORY_LIMIT = 5;
const RELEASE_SCAN_LIMIT = 10;
const CACHE_TTL_MS = 10 * 60 * 1000;
const REQUEST_TIMEOUT_MS = 10_000;
const MAX_RELEASES_BODY_CHARS = 512 * 1024;
const MAX_DIGEST_BODY_CHARS = 128 * 1024;

function trimTrailingSlash(value) {
  return String(value || "").replace(/\/+$/, "");
}

/**
 * 从环境变量推导发布源。owner/repo 同时给出时拼 GitHub 的 releases API
 * 和 asset base；什么都不配时返回空串，调用方据此跳过在线加载。
 */
function defaultReleaseSourceUrls(env = process.env) {
  const releasesApiUrl = String(env.LINGXI_UPDATE_RELEASES_API_URL || "").trim();
  const assetBaseUrl = String(env.LINGXI_UPDATE_DIGEST_BASE_URL || "").trim();
  const owner = String(env.LINGXI_UPDATE_GITHUB_OWNER || "").trim();
  const repo = String(env.LINGXI_UPDATE_GITHUB_REPO || "").trim();
  const githubConfigured = Boolean(owner && repo);
  return {
    releasesApiUrl: releasesApiUrl
      || (githubConfigured ? `https://api.github.com/repos/${owner}/${repo}/releases?per_page=20&page=1` : ""),
    releaseAssetBaseUrl: assetBaseUrl
      || (githubConfigured ? `https://github.com/${owner}/${repo}/releases/download` : ""),
  };
}

function versionFromTag(tag) {
  const match = /^v(\d+\.\d+\.\d+)$/.exec(String(tag || "").trim());
  return match ? match[1] : null;
}

function hasDigestAsset(release) {
  return Array.isArray(release?.assets)
    && release.assets.some((asset) => asset?.name === DIGEST_ASSET_NAME);
}

function digestUrl(tag, releaseAssetBaseUrl) {
  const base = String(releaseAssetBaseUrl || "").trim();
  if (!base) return null;
  const version = String(tag).replace(/^v/, "");
  if (base.includes("{tag}") || base.includes("{version}") || base.includes("{asset}")) {
    return base
      .replaceAll("{tag}", encodeURIComponent(tag))
      .replaceAll("{version}", encodeURIComponent(version))
      .replaceAll("{asset}", encodeURIComponent(DIGEST_ASSET_NAME));
  }
  return `${trimTrailingSlash(base)}/${encodeURIComponent(tag)}/${DIGEST_ASSET_NAME}`;
}

async function fetchJson(fetchImpl, url, { maxChars, timeoutMs }) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetchImpl(url, {
      headers: {
        Accept: "application/vnd.github+json",
        "User-Agent": "LingxiAgent-update-history",
      },
      signal: controller.signal,
    });
    if (!response?.ok) {
      throw new Error(`request failed (${response?.status || "unknown"}) for ${url}`);
    }
    const text = await response.text();
    if (text.length > maxChars) {
      throw new Error(`response too large for ${url}`);
    }
    return JSON.parse(text);
  } finally {
    clearTimeout(timer);
  }
}

async function loadOnlineEntries({ fetchImpl, normalize, timeoutMs, releasesApiUrl, releaseAssetBaseUrl }) {
  const releases = await fetchJson(fetchImpl, releasesApiUrl, {
    maxChars: MAX_RELEASES_BODY_CHARS,
    timeoutMs,
  });
  if (!Array.isArray(releases)) {
    throw new Error("GitHub releases response is not an array");
  }

  const candidates = releases
    .filter((release) => release && release.draft === false)
    .filter((release) => versionFromTag(release.tag_name) && hasDigestAsset(release))
    .slice(0, RELEASE_SCAN_LIMIT);

  const settled = await Promise.all(candidates.map(async (release) => {
    const version = versionFromTag(release.tag_name);
    if (!version) return null;
    const url = digestUrl(release.tag_name, releaseAssetBaseUrl);
    if (!url) return null;
    try {
      const payload = await fetchJson(fetchImpl, url, {
        maxChars: MAX_DIGEST_BODY_CHARS,
        timeoutMs,
      });
      return normalize(payload, version);
    } catch {
      return null;
    }
  }));

  return settled.filter(Boolean).slice(0, HISTORY_LIMIT);
}

function createUpdateDigestHistoryLoader({
  fetchImpl = globalThis.fetch,
  normalize,
  readBundledEntries,
  log = () => {},
  now = () => Date.now(),
  cacheTtlMs = CACHE_TTL_MS,
  timeoutMs = REQUEST_TIMEOUT_MS,
  releasesApiUrl,
  releaseAssetBaseUrl,
  env = process.env,
} = {}) {
  if (typeof fetchImpl !== "function") throw new TypeError("fetchImpl must be a function");
  if (typeof normalize !== "function") throw new TypeError("normalize must be a function");
  if (typeof readBundledEntries !== "function") throw new TypeError("readBundledEntries must be a function");

  const defaults = defaultReleaseSourceUrls(env);
  const resolvedReleasesApiUrl = trimTrailingSlash(releasesApiUrl) || defaults.releasesApiUrl;
  const resolvedAssetBaseUrl = trimTrailingSlash(releaseAssetBaseUrl) || defaults.releaseAssetBaseUrl;

  let cached = null;
  let inFlight = null;

  return async function loadUpdateDigestHistory() {
    const currentTime = now();
    if (cached && currentTime - cached.storedAt < cacheTtlMs) return cached.result;
    if (inFlight) return inFlight;

    inFlight = (async () => {
      try {
        if (!resolvedReleasesApiUrl) throw new Error("no release source configured");
        const entries = await loadOnlineEntries({
          fetchImpl,
          normalize,
          timeoutMs,
          releasesApiUrl: resolvedReleasesApiUrl,
          releaseAssetBaseUrl: resolvedAssetBaseUrl,
        });
        if (entries.length === 0) throw new Error("no valid release digests found");
        const result = {
          entries,
          source: "online",
          complete: entries.length === HISTORY_LIMIT,
        };
        cached = { result, storedAt: now() };
        return result;
      } catch (error) {
        log(`update history online load failed: ${error?.message || String(error)}`);
        const entries = readBundledEntries().slice(0, HISTORY_LIMIT);
        return {
          entries,
          source: entries.length > 0 ? "bundled" : "none",
          complete: false,
        };
      } finally {
        inFlight = null;
      }
    })();

    return inFlight;
  };
}

module.exports = {
  createUpdateDigestHistoryLoader,
  versionFromTag,
};
