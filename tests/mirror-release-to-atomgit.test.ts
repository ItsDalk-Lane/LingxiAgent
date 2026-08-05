import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import {
  buildAtomGitReleasePayload,
  getGithubLatestTag,
  listAtomGitReleases,
  mirrorRelease,
  normalizeUploadUrlPayload,
  parseArgs,
  run,
  selectGithubReleases,
} from "../scripts/mirror-release-to-atomgit.mjs";

const mirrorOptions = {
  githubOwner: "liliMozi",
  githubRepo: "openhanako",
  atomgitOwner: "liliMozi",
  atomgitRepo: "OpenHanako-Releases",
  dryRun: false,
};

function atomgitProjectLookupResponse(url: string, init: RequestInit = {}) {
  if (url !== "https://gitcode.com/api/v2/projects/liliMozi%2FOpenHanako-Releases?view=all") return null;
  expect(init.headers).toEqual(expect.objectContaining({
    Authorization: "Bearer atomgit-token",
    "X-Platform": "web",
    "X-App-Channel": "gitcode-fe",
  }));
  return new Response(JSON.stringify({ id: 10296556 }), { status: 200 });
}

function githubRelease(tagName: string, prerelease = true) {
  return {
    tag_name: tagName,
    target_commitish: "main",
    name: tagName,
    body: "Release notes",
    draft: false,
    prerelease,
    assets: [
      { name: "latest.yml", size: 120, browser_download_url: "https://example.com/latest.yml" },
      { name: "release-digest.v1.json", size: 240, browser_download_url: "https://example.com/release-digest.v1.json" },
    ],
  };
}

describe("mirror-release-to-atomgit", () => {
  it("defaults manual mirroring to the newest one release", () => {
    expect(parseArgs([], { GITHUB_REPOSITORY: "liliMozi/openhanako" })).toEqual(expect.objectContaining({
      githubOwner: "liliMozi",
      githubRepo: "openhanako",
      atomgitOwner: "liliMozi",
      atomgitRepo: "OpenHanako-Releases",
      selection: "newest",
      latest: 1,
    }));
  });

  it("selects newest published releases from GitHub, including prereleases", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(JSON.stringify([
        githubRelease("v0.425.4", true),
        { ...githubRelease("v0.425.3", false), draft: true },
      ])),
    });

    const releases = await selectGithubReleases({
      githubOwner: "liliMozi",
      githubRepo: "openhanako",
      latest: 1,
    }, { env: {}, fetchImpl });

    expect(releases).toHaveLength(1);
    expect(releases[0].tag_name).toBe("v0.425.4");
  });

  it("can select stable releases without prereleases", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      text: vi.fn().mockResolvedValue(JSON.stringify([
        githubRelease("v0.425.4", true),
        githubRelease("v0.425.3", true),
        githubRelease("v0.425.2", false),
      ])),
    });

    const releases = await selectGithubReleases({
      githubOwner: "liliMozi",
      githubRepo: "openhanako",
      selection: "stable",
      latest: 1,
    }, { env: {}, fetchImpl });

    expect(releases).toHaveLength(1);
    expect(releases[0].tag_name).toBe("v0.425.2");
  });

  it("runs multi-release selections oldest-to-newest so the newest target remains", async () => {
    const fetchImpl = vi.fn().mockResolvedValue(new Response(JSON.stringify([
      githubRelease("v0.425.4", true),
      githubRelease("v0.425.3", true),
    ]), { status: 200 }));
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    try {
      const summaries = await run(["--newest", "2", "--dry-run"], {
        env: { GITHUB_REPOSITORY: "liliMozi/openhanako" },
        fetchImpl,
      });
      expect(summaries.map(summary => summary.tag)).toEqual(["v0.425.3", "v0.425.4"]);
    } finally {
      log.mockRestore();
    }
  });

  it("maps GitHub prerelease and Latest state to AtomGit release_status", () => {
    expect(buildAtomGitReleasePayload(githubRelease("v0.425.4", true))).toEqual(expect.objectContaining({
      tag_name: "v0.425.4",
      draft: false,
      prerelease: true,
      release_status: "pre",
    }));
    expect(buildAtomGitReleasePayload(githubRelease("v0.425.3", false), "v0.425.3")).toEqual(expect.objectContaining({
      release_status: "latest",
    }));
    expect(buildAtomGitReleasePayload(githubRelease("v0.425.2", false), "v0.425.3")).not.toHaveProperty("release_status");
  });

  it("queries GitHub's authoritative latest stable release", async () => {
    const fetchImpl = vi.fn().mockResolvedValue(new Response(JSON.stringify(githubRelease("v0.425.3", false)), { status: 200 }));
    await expect(getGithubLatestTag(mirrorOptions, { env: {}, fetchImpl })).resolves.toBe("v0.425.3");
    expect(fetchImpl).toHaveBeenCalledWith(
      "https://api.github.com/repos/liliMozi/openhanako/releases/latest",
      expect.anything(),
    );
  });

  it("dry-runs without requiring AtomGit token or uploading assets", async () => {
    const result = await mirrorRelease({ dryRun: true }, githubRelease("v0.425.4"), {
      env: {},
      fetchImpl: vi.fn(),
    });
    expect(result).toEqual({
      tag: "v0.425.4",
      dryRun: true,
      prerelease: true,
      assetNames: ["latest.yml", "release-digest.v1.json"],
    });
  });

  it("uploads assets using the GitCode upload URL contract", async () => {
    const uploadBodies: unknown[] = [];
    let releaseListCalls = 0;
    const fetchImpl = vi.fn(async (input, init = {}) => {
      const url = String(input);
      const method = init.method || "GET";

      if (url.endsWith("/repos/liliMozi/openhanako/releases/latest")) {
        return new Response(JSON.stringify(githubRelease("v0.425.3", false)), { status: 200 });
      }

      const projectLookup = atomgitProjectLookupResponse(url, init);
      if (projectLookup) return projectLookup;

      if (url.includes("/repos/liliMozi/OpenHanako-Releases/releases?") && method === "GET") {
        releaseListCalls += 1;
        return new Response(JSON.stringify(releaseListCalls === 1 ? [] : [{
          tag_name: "v0.425.4",
          prerelease: true,
          created_at: "2026-07-21T00:00:00Z",
        }]), { status: 200 });
      }

      if (url.includes("/releases/v0.425.4/upload_url")) {
        const parsed = new URL(url);
        expect(parsed.searchParams.get("file_name")).toMatch(/latest\.yml|release-digest\.v1\.json/);
        expect(parsed.searchParams.has("file_size")).toBe(false);
        return new Response(JSON.stringify({
          url: `https://upload.example.com/${parsed.searchParams.get("file_name")}`,
          headers: {
            "Content-Type": "application/octet-stream",
            "x-upload-token": "upload-token",
          },
        }), { status: 200 });
      }

      if (url.startsWith("https://upload.example.com/")) {
        expect(method).toBe("PUT");
        uploadBodies.push(init.body);
        expect(init.headers).toEqual(expect.objectContaining({
          "Content-Type": "application/octet-stream",
          "x-upload-token": "upload-token",
        }));
        return new Response("", { status: 200 });
      }

      if (url.startsWith("https://example.com/")) {
        return new Response(`bytes:${pathBasename(url)}`, { status: 200 });
      }

      if (url.includes("/attach_files/latest.yml/download") && method === "HEAD") {
        return new Response(null, { status: 200, headers: { "content-length": "120" } });
      }

      if (url.includes("/attach_files/release-digest.v1.json/download") && method === "HEAD") {
        return new Response(null, { status: 200, headers: { "content-length": "240" } });
      }

      if (url.includes("/repos/liliMozi/OpenHanako-Releases/releases") && method === "POST") {
        return new Response(JSON.stringify({ tag_name: "v0.425.4" }), { status: 201 });
      }

      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    const result = await mirrorRelease(mirrorOptions, githubRelease("v0.425.4"), {
      env: { ATOMGIT_TOKEN: "atomgit-token" },
      fetchImpl,
    });

    expect(result.dryRun).toBe(false);
    expect(uploadBodies).toHaveLength(2);
    expect(uploadBodies.every(body => Buffer.isBuffer(body))).toBe(true);
  });

  it("skips already mirrored assets only after verifying their size", async () => {
    const existingAssets = [
      { name: "latest.yml", type: "attach" },
      { name: "release-digest.v1.json", type: "attach" },
    ];
    const fetchImpl = vi.fn(async (input, init = {}) => {
      const url = String(input);
      const method = init.method || "GET";

      if (url.endsWith("/repos/liliMozi/openhanako/releases/latest")) {
        return new Response(JSON.stringify(githubRelease("v0.425.3", false)), { status: 200 });
      }

      const projectLookup = atomgitProjectLookupResponse(url, init);
      if (projectLookup) return projectLookup;

      if (url.includes("/repos/liliMozi/OpenHanako-Releases/releases?") && method === "GET") {
        return new Response(JSON.stringify([{
          tag_name: "v0.425.4",
          prerelease: true,
          created_at: "2026-07-21T00:00:00Z",
          assets: existingAssets,
        }]), { status: 200 });
      }

      if (url.includes("/attach_files/latest.yml/download") && method === "HEAD") {
        return new Response(null, { status: 200, headers: { "content-length": "120" } });
      }

      if (url.includes("/attach_files/release-digest.v1.json/download") && method === "HEAD") {
        return new Response(null, { status: 200, headers: { "content-length": "240" } });
      }

      if (url.includes("/repos/liliMozi/OpenHanako-Releases/releases/v0.425.4") && method === "PATCH") {
        expect(JSON.parse(String(init.body))).toEqual(expect.objectContaining({
          release_status: "pre",
        }));
        return new Response(JSON.stringify({ tag_name: "v0.425.4", assets: existingAssets }), { status: 200 });
      }

      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    const result = await mirrorRelease(mirrorOptions, githubRelease("v0.425.4"), {
      env: { ATOMGIT_TOKEN: "atomgit-token" },
      fetchImpl,
    });

    expect(result.dryRun).toBe(false);
    expect(fetchImpl).not.toHaveBeenCalledWith(expect.stringContaining("/upload_url"), expect.anything());
  });

  it("keeps the newest fallback until a quota-blocked target is uploaded and verified", async () => {
    const target = {
      ...githubRelease("v0.425.4", true),
      assets: [githubRelease("v0.425.4", true).assets[0]],
    };
    const existing = Array.from({ length: 20 }, (_, index) => ({
      tag_name: `v0.424.${index + 1}`,
      prerelease: true,
      release_status: "pre",
      created_at: `2026-07-${String(index + 1).padStart(2, "0")}T00:00:00Z`,
      assets: [],
    }));
    const fallbackTag = "v0.424.20";
    const operations: string[] = [];
    let releaseListCalls = 0;
    const fetchImpl = vi.fn(async (input, init = {}) => {
      const url = String(input);
      const method = init.method || "GET";

      if (url.endsWith("/repos/liliMozi/openhanako/releases/latest")) {
        return new Response(JSON.stringify(githubRelease("v0.425.3", false)), { status: 200 });
      }
      const projectLookup = atomgitProjectLookupResponse(url, init);
      if (projectLookup) return projectLookup;
      if (url.includes("/repos/liliMozi/OpenHanako-Releases/releases?") && method === "GET") {
        releaseListCalls += 1;
        operations.push(`list:${releaseListCalls}`);
        const listed = releaseListCalls === 1
          ? existing
          : releaseListCalls === 2
            ? []
            : [existing.at(-1), { tag_name: target.tag_name, prerelease: true, created_at: "2026-07-21T00:00:00Z" }];
        return new Response(JSON.stringify(listed), { status: 200 });
      }
      if (url.includes("/api/v2/projects/10296556/releases/") && method === "DELETE") {
        expect(init.headers).toEqual(expect.objectContaining({ Authorization: "Bearer atomgit-token" }));
        expect(JSON.parse(String(init.body))).toEqual(expect.objectContaining({ project_id: 10296556 }));
        const tag = decodeURIComponent(new URL(url).pathname.split("/").pop() || "");
        operations.push(`delete:${tag}`);
        return new Response(null, { status: 204 });
      }
      if (url.endsWith("/repos/liliMozi/OpenHanako-Releases/releases?access_token=atomgit-token") && method === "POST") {
        operations.push("create:target");
        return new Response(JSON.stringify({ tag_name: target.tag_name, assets: [] }), { status: 201 });
      }
      if (url.includes(`/releases/${target.tag_name}/upload_url`)) {
        return new Response(JSON.stringify({ url: "https://upload.example.com/latest.yml" }), { status: 200 });
      }
      if (url === "https://example.com/latest.yml") {
        return new Response("asset", { status: 200 });
      }
      if (url === "https://upload.example.com/latest.yml" && method === "PUT") {
        operations.push("upload:target");
        return new Response(null, { status: 200 });
      }
      if (url.includes("/attach_files/latest.yml/download") && method === "HEAD") {
        operations.push("verify:target");
        return new Response(null, { status: 200, headers: { "content-length": "120" } });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    await mirrorRelease(mirrorOptions, target, {
      env: { ATOMGIT_TOKEN: "atomgit-token" },
      fetchImpl,
    });

    expect(operations.indexOf(`delete:${fallbackTag}`)).toBeGreaterThan(operations.indexOf("verify:target"));
    expect(operations.indexOf("create:target")).toBeGreaterThan(operations.indexOf("delete:v0.424.19"));
    expect(operations.at(-1)).toBe(`delete:${fallbackTag}`);
    expect(fetchImpl).not.toHaveBeenCalledWith(
      expect.stringContaining("/repository/tags/"),
      expect.objectContaining({ method: "DELETE" }),
    );
  });

  it("does not delete the fallback when target asset verification fails", async () => {
    const fallback = {
      tag_name: "v0.425.3",
      prerelease: true,
      release_status: "pre",
      created_at: "2026-07-20T00:00:00Z",
      assets: [],
    };
    const target = {
      ...githubRelease("v0.425.4", true),
      assets: [githubRelease("v0.425.4", true).assets[0]],
    };
    const methods: string[] = [];
    const fetchImpl = vi.fn(async (input, init = {}) => {
      const url = String(input);
      const method = init.method || "GET";
      methods.push(method);

      if (url.endsWith("/repos/liliMozi/openhanako/releases/latest")) {
        return new Response(JSON.stringify(githubRelease("v0.425.2", false)), { status: 200 });
      }
      const projectLookup = atomgitProjectLookupResponse(url, init);
      if (projectLookup) return projectLookup;
      if (url.includes("/repos/liliMozi/OpenHanako-Releases/releases?") && method === "GET") {
        return new Response(JSON.stringify([fallback]), { status: 200 });
      }
      if (url.endsWith("/repos/liliMozi/OpenHanako-Releases/releases?access_token=atomgit-token") && method === "POST") {
        return new Response(JSON.stringify({ tag_name: target.tag_name, assets: [] }), { status: 201 });
      }
      if (url.includes(`/releases/${target.tag_name}/upload_url`)) {
        return new Response(JSON.stringify({ url: "https://upload.example.com/latest.yml" }), { status: 200 });
      }
      if (url === "https://example.com/latest.yml") return new Response("asset", { status: 200 });
      if (url === "https://upload.example.com/latest.yml") return new Response(null, { status: 200 });
      if (url.includes("/attach_files/latest.yml/download") && method === "HEAD") {
        return new Response("not ready", { status: 503 });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    await expect(mirrorRelease(mirrorOptions, target, {
      env: { ATOMGIT_TOKEN: "atomgit-token" },
      fetchImpl,
    })).rejects.toThrow(/size check failed/);
    expect(methods).not.toContain("DELETE");
  });

  it("fails closed when AtomGit pagination cannot be completed", async () => {
    const firstPage = Array.from({ length: 20 }, (_, index) => ({
      tag_name: `v0.400.${index}`,
      prerelease: true,
      created_at: "2026-07-01T00:00:00Z",
    }));
    const methods: string[] = [];
    const fetchImpl = vi.fn(async (input, init = {}) => {
      const url = String(input);
      const method = init.method || "GET";
      methods.push(method);
      if (url.endsWith("/repos/liliMozi/openhanako/releases/latest")) {
        return new Response(JSON.stringify(githubRelease("v0.425.3", false)), { status: 200 });
      }
      const projectLookup = atomgitProjectLookupResponse(url, init);
      if (projectLookup) return projectLookup;
      const page = new URL(url).searchParams.get("page");
      if (page === "1") return new Response(JSON.stringify(firstPage), { status: 200 });
      if (page === "2") return new Response("upstream unavailable", { status: 503 });
      throw new Error(`unexpected fetch: ${method} ${url}`);
    });

    await expect(mirrorRelease(mirrorOptions, githubRelease("v0.425.4"), {
      env: { ATOMGIT_TOKEN: "atomgit-token" },
      fetchImpl,
    })).rejects.toThrow(/release list page 2 failed/);
    expect(methods).not.toContain("DELETE");
    expect(methods).not.toContain("POST");
  });

  it("wires manual GitHub release changes to exact-tag mirroring", () => {
    const workflow = readFileSync(new URL("../.github/workflows/mirror-release-to-atomgit.yml", import.meta.url), "utf8");
    expect(workflow).toContain("release:");
    expect(workflow).toContain("- published");
    expect(workflow).toContain("- edited");
    expect(workflow).toContain("- prereleased");
    expect(workflow).toContain("- released");
    expect(workflow).toContain("RELEASE_TAG: ${{ github.event.release.tag_name }}");
    expect(workflow).toContain("ARGS+=(--tag \"$RELEASE_TAG\")");
    expect(workflow).toContain("github.event.sender.login != 'github-actions[bot]'");
    expect(workflow).toContain("group: atomgit-release-mirror");
    expect(workflow).toContain("cancel-in-progress: false");
  });

  it("rejects duplicate tags across AtomGit list pages", async () => {
    const fullPage = Array.from({ length: 20 }, (_, index) => ({ tag_name: `v0.1.${index}` }));
    const fetchImpl = vi.fn(async (input) => {
      const url = String(input);
      const page = new URL(url).searchParams.get("page");
      return new Response(JSON.stringify(page === "1" ? fullPage : [{ tag_name: "v0.1.0" }]), { status: 200 });
    });
    await expect(listAtomGitReleases(mirrorOptions, {
      env: { ATOMGIT_TOKEN: "token" },
      fetchImpl,
    })).rejects.toThrow(/duplicate tag/);
  });

  it("normalizes AtomGit upload URL responses from known shapes", () => {
    expect(normalizeUploadUrlPayload({ upload_url: "https://upload.example.com", headers: { "x-token": "a" } })).toEqual({
      uploadUrl: "https://upload.example.com",
      headers: { "x-token": "a" },
    });
    expect(() => normalizeUploadUrlPayload({})).toThrow(/upload URL/);
  });
});

function pathBasename(url: string) {
  return new URL(url).pathname.split("/").pop() || "asset";
}
