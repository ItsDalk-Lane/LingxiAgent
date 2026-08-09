import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { appendDigestFileToHistoryFile, generateDigestWithDeepSeek, parseArgs } from "../scripts/generate-release-digest.mjs";

describe("generate-release-digest", () => {
  it("parses local pre-tag defaults without requiring release lookup", () => {
    const args = parseArgs(["--out", "tmp/digest.json"], {
      GITHUB_REF_NAME: "v0.425.4",
      GITHUB_REPOSITORY: "ItsDalk-Lane/LingxiAgent",
    });
    expect(args).toEqual(expect.objectContaining({
      tag: "v0.425.4",
      previousTag: "auto",
      ref: "HEAD",
      owner: "ItsDalk-Lane",
      repo: "LingxiAgent",
      out: "tmp/digest.json",
      model: "deepseek-v4-flash",
    }));
  });

  it("allows DEEPSEEK_MODEL to override the default model", () => {
    const args = parseArgs(["--tag", "v0.425.4"], {
      DEEPSEEK_MODEL: "deepseek-v4-flash-canary",
    });

    expect(args.model).toBe("deepseek-v4-flash-canary");
  });

  it("accepts an explicit git ref and local release notes file", () => {
    const args = parseArgs([
      "--tag", "v0.425.4",
      "--ref", "HEAD",
      "--release-notes-file", "notes.md",
    ], {});

    expect(args).toEqual(expect.objectContaining({
      tag: "v0.425.4",
      ref: "HEAD",
      releaseNotesFile: "notes.md",
    }));
  });

  it("requests JSON schema output from DeepSeek Responses API", async () => {
    const digest = {
      schemaVersion: 1,
      tag: "v0.425.4",
      version: "0.425.4",
      previousTag: "v0.425.3",
      generatedAt: "2026-07-05T00:00:00.000Z",
      noUserFacingChanges: false,
      summary: { zh: "更新说明更清楚。", en: "Update notes are clearer." },
      counts: { feature: 1, fix: 0, improvement: 0, migration: 0 },
      source: {
        owner: "ItsDalk-Lane",
        repo: "LingxiAgent",
        commitRange: "v0.425.3..v0.425.4",
        releaseUrl: "https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v0.425.4",
        releaseNotes: "",
      },
      items: [
        {
          id: "digest",
          kind: "feature",
          importance: "high",
          title: { zh: "更新摘要", en: "Update digest" },
          summary: { zh: "About 页展示更新内容。", en: "The About page shows update content." },
          details: [],
          sources: [{ type: "commit", ref: "abc123", title: "Add digest" }],
        },
      ],
    };
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({
        status: "completed",
        output: [
          { type: "reasoning", content: [{ type: "reasoning_text", text: "The user wants a release digest." }] },
          { type: "message", content: [{ type: "output_text", text: JSON.stringify(digest) }] },
        ],
      }),
    });

    const result = await generateDigestWithDeepSeek(
      { tag: "v0.425.4", version: "0.425.4", commits: [] },
      {
        env: { DEEPSEEK_API_KEY: "test-key" },
        fetchImpl,
        model: "deepseek-v4-flash",
      },
    );

    expect(result.tag).toBe("v0.425.4");
    expect(fetchImpl).toHaveBeenCalledWith("https://api.deepseek.com/responses", expect.objectContaining({
      method: "POST",
      headers: expect.objectContaining({ Authorization: "Bearer test-key" }),
    }));
    const body = JSON.parse(fetchImpl.mock.calls[0][1].body);
    expect(body.model).toBe("deepseek-v4-flash");
    expect(body.text.format).toEqual(expect.objectContaining({
      type: "json_schema",
      name: "hana_release_digest",
      schema: expect.any(Object),
    }));
    expect(body.text.format).not.toHaveProperty("strict");
    expect(body).not.toHaveProperty("store");
    expect(body.max_output_tokens).toBe(8000);
  });

  it("requires DEEPSEEK_API_KEY before making a request", async () => {
    const fetchImpl = vi.fn();

    await expect(generateDigestWithDeepSeek(
      { tag: "v0.425.4", version: "0.425.4", commits: [] },
      { env: {}, fetchImpl },
    )).rejects.toThrow("DEEPSEEK_API_KEY is required");
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("rejects an incomplete DeepSeek response even when it contains partial JSON", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({
        status: "incomplete",
        incomplete_details: { reason: "max_output_tokens" },
        output_text: JSON.stringify(digestFixture("0.425.4", "0.425.3")),
      }),
    });

    await expect(generateDigestWithDeepSeek(
      { tag: "v0.425.4", version: "0.425.4", commits: [] },
      { env: { DEEPSEEK_API_KEY: "test-key" }, fetchImpl },
    )).rejects.toThrow(/incomplete.*max_output_tokens/i);
  });

  it("does not include a DeepSeek error body in the thrown error", async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      text: vi.fn().mockResolvedValue("server echoed test-key"),
    });

    const operation = generateDigestWithDeepSeek(
      { tag: "v0.425.4", version: "0.425.4", commits: [] },
      { env: { DEEPSEEK_API_KEY: "test-key" }, fetchImpl },
    );
    await expect(operation).rejects.toThrow("HTTP 401");
    await expect(operation).rejects.not.toThrow(/test-key/);
  });

  it("rejects a digest for a different release", async () => {
    const digest = digestFixture("0.425.5", "0.425.4");
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      json: vi.fn().mockResolvedValue({ status: "completed", output_text: JSON.stringify(digest) }),
    });

    await expect(generateDigestWithDeepSeek(
      { tag: "v0.425.4", version: "0.425.4", commits: [] },
      { env: { DEEPSEEK_API_KEY: "test-key" }, fetchImpl },
    )).rejects.toThrow(/tag.*v0\.425\.4.*v0\.425\.5/i);
  });
});

function digestFixture(version: string, previous: string) {
  return {
    schemaVersion: 1,
    tag: `v${version}`,
    version,
    previousTag: `v${previous}`,
    generatedAt: "2026-07-05T00:00:00.000Z",
    noUserFacingChanges: false,
    summary: { zh: "更新说明更清楚。", en: "Update notes are clearer." },
    counts: { feature: 1, fix: 0, improvement: 0, migration: 0 },
    source: {
      owner: "ItsDalk-Lane",
      repo: "LingxiAgent",
      commitRange: `v${previous}..v${version}`,
      releaseUrl: `https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v${version}`,
      releaseNotes: "",
    },
    items: [
      {
        id: "digest",
        kind: "feature",
        importance: "high",
        title: { zh: "更新摘要", en: "Update digest" },
        summary: { zh: "About 页展示更新内容。", en: "The About page shows update content." },
        details: [],
        sources: [{ type: "commit", ref: "abc123", title: "Add digest" }],
      },
    ],
  };
}

describe("appendDigestFileToHistoryFile（手写工作流：追加一节进 v2 史册）", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "digest-history-"));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("史册不存在时创建单条 v2 史册", async () => {
    const digestPath = path.join(tmpDir, "release-digest.v1.json");
    const historyPath = path.join(tmpDir, "release-digest.v2.json");
    fs.writeFileSync(digestPath, JSON.stringify(digestFixture("0.425.4", "0.425.3")));

    await appendDigestFileToHistoryFile(digestPath, historyPath);

    const history = JSON.parse(fs.readFileSync(historyPath, "utf-8"));
    expect(history.schema).toBe(2);
    expect(history.entries.map((entry: { version: string }) => entry.version)).toEqual(["0.425.4"]);
  });

  it("已有史册时新版本插到头部，老 entries 原样保留", async () => {
    const digestPath = path.join(tmpDir, "release-digest.v1.json");
    const historyPath = path.join(tmpDir, "release-digest.v2.json");
    const oldEntry = digestFixture("0.425.3", "0.425.2");
    fs.writeFileSync(historyPath, JSON.stringify({ schema: 2, entries: [oldEntry] }));
    fs.writeFileSync(digestPath, JSON.stringify(digestFixture("0.425.4", "0.425.3")));

    await appendDigestFileToHistoryFile(digestPath, historyPath);

    const history = JSON.parse(fs.readFileSync(historyPath, "utf-8"));
    expect(history.entries.map((entry: { version: string }) => entry.version)).toEqual(["0.425.4", "0.425.3"]);
    expect(history.entries[1]).toEqual(oldEntry);
  });

  it("同版本重跑覆盖头部条目（幂等修订）", async () => {
    const digestPath = path.join(tmpDir, "release-digest.v1.json");
    const historyPath = path.join(tmpDir, "release-digest.v2.json");
    fs.writeFileSync(historyPath, JSON.stringify({ schema: 2, entries: [digestFixture("0.425.4", "0.425.3")] }));
    const revised = digestFixture("0.425.4", "0.425.3");
    revised.summary = { zh: "修订后的摘要。", en: "Revised summary." };
    fs.writeFileSync(digestPath, JSON.stringify(revised));

    await appendDigestFileToHistoryFile(digestPath, historyPath);

    const history = JSON.parse(fs.readFileSync(historyPath, "utf-8"));
    expect(history.entries).toHaveLength(1);
    expect(history.entries[0].summary.en).toBe("Revised summary.");
  });

  it("旧版本拒绝追加（防止倒灌）", async () => {
    const digestPath = path.join(tmpDir, "release-digest.v1.json");
    const historyPath = path.join(tmpDir, "release-digest.v2.json");
    fs.writeFileSync(historyPath, JSON.stringify({ schema: 2, entries: [digestFixture("0.425.4", "0.425.3")] }));
    fs.writeFileSync(digestPath, JSON.stringify(digestFixture("0.425.3", "0.425.2")));

    await expect(appendDigestFileToHistoryFile(digestPath, historyPath)).rejects.toThrow(/older|decreasing|head/i);
  });

  it("parseArgs 支持 --append-history / --history-file", () => {
    const args = parseArgs(["--tag", "v0.425.4", "--append-history", "--history-file", "tmp/history.json"], {});
    expect(args).toEqual(expect.objectContaining({
      appendHistory: true,
      historyFile: "tmp/history.json",
    }));
  });

  it("--append-history 模式不要求 --tag（append 路径不接触 git/LLM）", () => {
    const args = parseArgs(["--append-history"], {});
    expect(args.appendHistory).toBe(true);
    expect(args.historyFile).toBe("release-digest.v2.json");
  });
});
