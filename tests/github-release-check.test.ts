import { createRequire } from "module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const { checkLatestRelease, parseVersionTriplet, compareVersions, RELEASES_LATEST_API } =
  require("../desktop/src/shared/github-release-check.cjs");

/** 造一个最小可用的 fetch mock：只关心 url、返回 ok + JSON 体。 */
function makeFetchMock(response: { ok?: boolean; status?: number; body?: unknown } | null) {
  return async (url: string) => {
    expect(url).toBe(RELEASES_LATEST_API);
    if (response === null) {
      throw new Error("network down");
    }
    const ok = response.ok !== false;
    return {
      ok,
      status: response.status ?? (ok ? 200 : 500),
      text: async () => JSON.stringify(response.body ?? {}),
    };
  };
}

describe("github-release-check — 纯函数", () => {
  it("parseVersionTriplet 解析 v 前缀与裸版本", () => {
    expect(parseVersionTriplet("v0.1.2")).toEqual([0, 1, 2]);
    expect(parseVersionTriplet("0.1.2")).toEqual([0, 1, 2]);
    expect(parseVersionTriplet("v1.100.0")).toEqual([1, 100, 0]);
    expect(parseVersionTriplet("nope")).toBeNull();
    expect(parseVersionTriplet("")).toBeNull();
  });

  it("compareVersions 按数值段比较，不是字符串比较", () => {
    // 关键回归：0.100.0 必须比 0.99.0 新（字符串比较会反）。
    expect(compareVersions("0.100.0", "0.99.0")).toBe(1);
    expect(compareVersions("0.99.0", "0.100.0")).toBe(-1);
    expect(compareVersions("0.1.2", "0.1.2")).toBe(0);
    expect(compareVersions("v1.2.3", "1.2.2")).toBe(1);
    expect(compareVersions("abc", "1.0.0")).toBeNull();
  });
});

describe("github-release-check — checkLatestRelease", () => {
  it("远端版本更新 → available，带 latestVersion 与 releaseUrl", async () => {
    const fetchImpl = makeFetchMock({
      body: {
        tag_name: "v0.2.0",
        html_url: "https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v0.2.0",
      },
    });
    const result = await checkLatestRelease({ currentVersion: "0.1.2", fetchImpl });
    expect(result).toEqual({
      status: "available",
      latestVersion: "0.2.0",
      releaseUrl: "https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v0.2.0",
    });
  });

  it("远端版本相同 → latest", async () => {
    const fetchImpl = makeFetchMock({ body: { tag_name: "v0.1.2", html_url: "https://x/y" } });
    const result = await checkLatestRelease({ currentVersion: "0.1.2", fetchImpl });
    expect(result.status).toBe("latest");
    expect(result.latestVersion).toBe("0.1.2");
  });

  it("HTTP 失败 → error，不抛异常", async () => {
    const fetchImpl = makeFetchMock({ ok: false, status: 403 });
    const result = await checkLatestRelease({ currentVersion: "0.1.2", fetchImpl });
    expect(result.status).toBe("error");
    expect(result.error).toMatch(/403/);
  });

  it("网络异常 → error（不抛，降级成可重试状态）", async () => {
    const fetchImpl = makeFetchMock(null);
    const result = await checkLatestRelease({ currentVersion: "0.1.2", fetchImpl });
    expect(result.status).toBe("error");
    expect(result.error).toBe("network");
  });

  it("tag 无法解析成版本号 → error（不静默当 latest）", async () => {
    const fetchImpl = makeFetchMock({ body: { tag_name: "nightly-2026", html_url: "https://x" } });
    const result = await checkLatestRelease({ currentVersion: "0.1.2", fetchImpl });
    expect(result.status).toBe("error");
    expect(result.error).toBe("unparsable tag");
  });

  it("fetch 不可用 → error（模拟 globalThis.fetch 缺失的罕见环境）", async () => {
    // 模块在 fetchImpl 未注入时回落到 globalThis.fetch；测试环境里后者
    // 总是存在的，所以这里临时把它摘掉，验证防御性守卫真的兜得住。
    const savedFetch = globalThis.fetch;
    (globalThis as { fetch?: unknown }).fetch = undefined;
    try {
      const result = await checkLatestRelease({ currentVersion: "0.1.2" });
      expect(result.status).toBe("error");
    } finally {
      globalThis.fetch = savedFetch;
    }
  });

  it("releaseUrl 缺失时 releaseUrl 为 null（不崩，调用方回退到 releases/latest 总入口）", async () => {
    const fetchImpl = makeFetchMock({ body: { tag_name: "v0.2.0" } });
    const result = await checkLatestRelease({ currentVersion: "0.1.2", fetchImpl });
    expect(result.status).toBe("available");
    expect(result.releaseUrl).toBeNull();
  });
});
