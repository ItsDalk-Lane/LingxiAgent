import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { fetchCitationGradeWebSnapshot } from "../lib/knowledge/web-snapshot-security.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];
const publicAddress = [{ address: "93.184.216.34", family: 4 as const }];

function htmlResponse(bytes = Buffer.from("<h1>Public</h1>", "utf8")) {
  return {
    status: 200,
    headers: { "content-type": "text/html; charset=utf-8" },
    bytes,
  };
}

afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe("网页来源网络边界", () => {
  it("在发出请求前拒绝凭证、非默认端口、本机、内网和保留地址", async () => {
    const requestOnce = vi.fn(async () => htmlResponse());
    const cases = [
      "http://user:password@example.com/",
      "https://example.com:444/",
      "http://127.0.0.1/",
      "http://2130706433/",
      "http://[::1]/",
      "http://[::ffff:127.0.0.1]/",
      "http://localhost/",
      "http://private.example/",
      "http://reserved.example/",
    ];

    for (const url of cases) {
      await expect(fetchCitationGradeWebSnapshot(url, {
        resolveHost: async hostname => {
          if (hostname === "127.0.0.1") return [{ address: hostname, family: 4 }];
          if (hostname === "::1" || hostname.startsWith("::ffff:")) {
            return [{ address: hostname, family: 6 }];
          }
          if (hostname === "localhost") return [{ address: "127.0.0.1", family: 4 }];
          if (hostname === "private.example") return [{ address: "10.20.30.40", family: 4 }];
          if (hostname === "reserved.example") return [{ address: "240.0.0.1", family: 4 }];
          return publicAddress;
        },
        requestOnce,
      })).rejects.toMatchObject({ code: "KNOWLEDGE_WEB_URL_BLOCKED" });
    }
    expect(requestOnce).not.toHaveBeenCalled();
  });

  it("每次跳转都重新解析并阻断跳向内网的响应", async () => {
    const requestOnce = vi.fn(async () => ({
      status: 302,
      headers: { location: "http://internal.example/admin" },
      bytes: Buffer.alloc(0),
    }));
    const resolveHost = vi.fn(async (hostname: string) => hostname === "internal.example"
      ? [{ address: "192.168.1.20", family: 4 as const }]
      : publicAddress);

    await expect(fetchCitationGradeWebSnapshot("https://example.com/start", {
      resolveHost,
      requestOnce,
    })).rejects.toMatchObject({ code: "KNOWLEDGE_WEB_URL_BLOCKED" });
    expect(requestOnce).toHaveBeenCalledTimes(1);
    expect(resolveHost).toHaveBeenNthCalledWith(2, "internal.example");
  });

  it("限制跳转次数、响应大小、内容类型和压缩编码", async () => {
    const redirect = vi.fn(async () => ({
      status: 302,
      headers: { location: "/again" },
      bytes: Buffer.alloc(0),
    }));
    await expect(fetchCitationGradeWebSnapshot("https://example.com/start", {
      resolveHost: async () => publicAddress,
      requestOnce: redirect,
    })).rejects.toMatchObject({ code: "KNOWLEDGE_WEB_FETCH_FAILED" });
    expect(redirect).toHaveBeenCalledTimes(6);

    await expect(fetchCitationGradeWebSnapshot("https://example.com/file", {
      maxBytes: 4,
      resolveHost: async () => publicAddress,
      requestOnce: async () => ({
        ...htmlResponse(Buffer.from("12345")),
        headers: { "content-type": "text/html", "content-length": "5" },
      }),
    })).rejects.toMatchObject({ code: "KNOWLEDGE_WEB_TOO_LARGE" });

    await expect(fetchCitationGradeWebSnapshot("https://example.com/file", {
      resolveHost: async () => publicAddress,
      requestOnce: async () => ({
        ...htmlResponse(Buffer.from("{}")),
        headers: { "content-type": "application/json" },
      }),
    })).rejects.toMatchObject({ code: "KNOWLEDGE_WEB_TYPE_UNSUPPORTED" });

    await expect(fetchCitationGradeWebSnapshot("https://example.com/file", {
      resolveHost: async () => publicAddress,
      requestOnce: async () => ({
        ...htmlResponse(),
        headers: { "content-type": "text/html", "content-encoding": "gzip" },
      }),
    })).rejects.toMatchObject({ code: "KNOWLEDGE_WEB_TYPE_UNSUPPORTED" });
  });

  it("固定解析后的公网地址并返回一次性冻结的 HTML 字节", async () => {
    const bytes = Buffer.from("<h1>Frozen page</h1><p>Version one.</p>", "utf8");
    const requestOnce = vi.fn(async (url: URL, address: { address: string }) => {
      expect(url.hostname).toBe("example.com");
      expect(address.address).toBe("93.184.216.34");
      return htmlResponse(bytes);
    });
    const result = await fetchCitationGradeWebSnapshot(
      "https://example.com/report?edition=1#section",
      {
        now: () => "2026-08-25T12:00:00.000Z",
        resolveHost: async () => publicAddress,
        requestOnce,
      },
    );

    expect(result).toMatchObject({
      originalUrl: "https://example.com/report?edition=1",
      finalUrl: "https://example.com/report?edition=1",
      mimeType: "text/html",
      fetchedAt: "2026-08-25T12:00:00.000Z",
    });
    expect(result.bytes).toEqual(bytes);
    expect(requestOnce).toHaveBeenCalledTimes(1);
  });
});

describe("网页与粘贴来源冻结", () => {
  it("网页只抓取一次，保存原始 HTML，并在重启后从同一快照恢复引用", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-web-"));
    tempDirs.push(root);
    const lingxiHome = path.join(root, "home");
    fs.mkdirSync(lingxiHome);
    const bytes = Buffer.from(
      "<html><body><h1>Frozen title</h1><p>Stable evidence.</p><script>deleteEverything()</script></body></html>",
      "utf8",
    );
    const fetchWebSnapshot = vi.fn(async () => ({
      originalUrl: "https://example.com/report?token=private",
      finalUrl: "https://example.com/report?token=private",
      mimeType: "text/html" as const,
      bytes,
      fetchedAt: "2026-08-25T12:00:00.000Z",
    }));
    const manager = new KnowledgeManager({ lingxiHome, fetchWebSnapshot });
    managers.push(manager);
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "网页" });
    const imported = await manager.importWebSnapshot({
      studioId: "studio-a",
      notebookId: notebook.id,
      url: "https://example.com/report?token=private",
    });
    const artifact = await manager.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    const blocks = manager.listArtifactBlocks({ studioId: "studio-a", parseArtifactId: artifact.id });
    expect(blocks.map(block => block.text)).toEqual(["Frozen title", "Stable evidence."]);
    expect(manager.readContentSnapshot({ studioId: "studio-a", snapshotId: imported.snapshot.id })).toEqual(bytes);
    const citation = manager.createCitation({
      studioId: "studio-a",
      parseArtifactId: artifact.id,
      blockId: blocks[1].id,
      startOffset: 0,
      endOffset: 15,
    });
    expect(fetchWebSnapshot).toHaveBeenCalledTimes(1);
    manager.close();
    managers.splice(managers.indexOf(manager), 1);

    const restarted = new KnowledgeManager({ lingxiHome });
    managers.push(restarted);
    expect(restarted.resolveCitation({ studioId: "studio-a", citationId: citation.id }))
      .toMatchObject({ citation: { canonicalText: "Stable evidence" }, snapshot: { id: imported.snapshot.id } });
    expect(restarted.readContentSnapshot({ studioId: "studio-a", snapshotId: imported.snapshot.id })).toEqual(bytes);
    expect(fetchWebSnapshot).toHaveBeenCalledTimes(1);
  });

  it("手动粘贴文本保留原始字节并产生稳定行锚点", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-paste-"));
    tempDirs.push(root);
    const lingxiHome = path.join(root, "home");
    fs.mkdirSync(lingxiHome);
    const manager = new KnowledgeManager({ lingxiHome });
    managers.push(manager);
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "粘贴" });
    const text = "第一行  \n第二行原文\n";
    const imported = await manager.importPastedText({
      studioId: "studio-a",
      notebookId: notebook.id,
      text,
      displayName: "现场记录",
    });
    const artifact = await manager.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    const blocks = manager.listArtifactBlocks({ studioId: "studio-a", parseArtifactId: artifact.id });

    expect(imported.source).toMatchObject({ sourceType: "pasted_text", displayName: "现场记录" });
    expect(manager.readContentSnapshot({ studioId: "studio-a", snapshotId: imported.snapshot.id }))
      .toEqual(Buffer.from(text, "utf8"));
    expect(blocks).toMatchObject([
      { text: "第一行", locator: { lineStart: 1, lineEnd: 1 } },
      { text: "第二行原文", locator: { lineStart: 2, lineEnd: 2 } },
    ]);
  });
});
