import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  KnowledgeManager,
  type KnowledgeManagerOptions,
} from "../lib/knowledge/knowledge-manager.ts";
import { authorizeHttpRoute, classifyHttpRoute } from "../server/http/route-security.ts";
import { createKnowledgeRoute } from "../server/routes/knowledge.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function localOwner(studioId = "studio-a") {
  return {
    kind: "local_user",
    userId: "user-a",
    studioId,
    serverId: "server-a",
    serverNodeId: "node-a",
    connectionKind: "local",
    credentialKind: "loopback_token",
    scopes: ["studio.owner"],
  };
}

function remoteOwner(studioId = "studio-a") {
  return {
    kind: "device",
    userId: "user-a",
    studioId,
    serverId: "server-a",
    serverNodeId: "node-a",
    deviceId: "device-a",
    connectionKind: "lan",
    credentialKind: "device_credential",
    trustState: "paired",
    scopes: ["studio.owner"],
  };
}

function appHarness(
  principal = localOwner(),
  managerOptions: Partial<Omit<KnowledgeManagerOptions, "lingxiHome">> = {},
) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-route-"));
  tempDirs.push(root);
  const lingxiHome = path.join(root, "home");
  const importsDir = path.join(root, "imports");
  fs.mkdirSync(lingxiHome);
  fs.mkdirSync(importsDir);
  const knowledge = new KnowledgeManager({ ...managerOptions, lingxiHome });
  managers.push(knowledge);
  const engine = {
    lingxiHome,
    knowledge,
    getRuntimeContext: () => ({
      userId: "user-a",
      studioId: "studio-a",
      serverId: "server-a",
      serverNodeId: "node-a",
      connectionKind: "local",
      credentialKind: "loopback_token",
      capabilities: ["studio.owner"],
    }),
  };
  const app = new Hono();
  app.use("*", async (c, next) => {
    (c as any).set("authPrincipal", Object.freeze(principal));
    await next();
  });
  app.route("/api", createKnowledgeRoute(engine));
  return { app, knowledge, importsDir };
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe("Knowledge route", () => {
  it("完成 Notebook、文件导入、解析、引用查看与托管原文读取", async () => {
    const { app, knowledge, importsDir } = appHarness();
    const createdResponse = await app.request("/api/knowledge/notebooks", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: "法规" }),
    });
    expect(createdResponse.status).toBe(201);
    const created = await createdResponse.json() as any;

    const inputPath = path.join(importsDir, "law.txt");
    fs.writeFileSync(inputPath, "第一条\n必须保留证据。\n", "utf-8");
    const importResponse = await app.request(
      `/api/knowledge/notebooks/${created.notebook.id}/sources`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ filePath: inputPath }),
      },
    );
    expect(importResponse.status).toBe(201);
    const imported = await importResponse.json() as any;
    expect(imported.parseArtifact.status).toBe("ready");
    expect(JSON.stringify(imported)).not.toContain(inputPath);
    expect(JSON.stringify(imported)).not.toContain("storagePath");

    const blocksResponse = await app.request(
      `/api/knowledge/parse-artifacts/${imported.parseArtifact.id}/blocks`,
    );
    expect(blocksResponse.status).toBe(200);
    const { blocks } = await blocksResponse.json() as any;
    const citation = knowledge.createCitation({
      studioId: "studio-a",
      parseArtifactId: imported.parseArtifact.id,
      blockId: blocks[1].id,
      startOffset: 2,
      endOffset: 6,
    });

    const citationResponse = await app.request(`/api/knowledge/citations/${citation.id}`);
    expect(citationResponse.status).toBe(200);
    const citationBody = await citationResponse.json() as any;
    expect(citationBody).toMatchObject({
      citation: { canonicalText: "保留证据" },
      viewer: { locator: { lineStart: 2 } },
    });
    expect(JSON.stringify(citationBody)).not.toContain("storagePath");
    expect(JSON.stringify(citationBody)).not.toContain(inputPath);

    fs.unlinkSync(inputPath);
    const contentResponse = await app.request(citationBody.viewer.contentUrl);
    expect(contentResponse.status).toBe(200);
    expect(contentResponse.headers.get("content-type")).toContain("text/plain");
    expect(await contentResponse.text()).toContain("必须保留证据");
  });

  it("远端 Owner 可读但不能让服务器按绝对路径导入，本 Studio 之外明确拒绝", async () => {
    const webBytes = Buffer.from(
      "<html><body><p>冻结网页证据</p><script>window.attack()</script></body></html>",
      "utf8",
    );
    const fetchWebSnapshot = vi.fn(async () => ({
      originalUrl: "https://example.com/report?token=secret",
      finalUrl: "https://example.com/report?token=secret",
      mimeType: "text/html" as const,
      bytes: webBytes,
      fetchedAt: "2026-08-25T12:00:00.000Z",
    }));
    const remote = appHarness(remoteOwner(), { fetchWebSnapshot });
    const notebook = remote.knowledge.createNotebook({ studioId: "studio-a", name: "远端" });
    const inputPath = path.join(remote.importsDir, "remote.txt");
    fs.writeFileSync(inputPath, "内容", "utf-8");
    const importResponse = await remote.app.request(
      `/api/knowledge/notebooks/${notebook.id}/sources`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ filePath: inputPath }),
      },
    );
    expect(importResponse.status).toBe(403);
    expect(await importResponse.json()).toMatchObject({ error: "KNOWLEDGE_LOCAL_IMPORT_REQUIRED" });

    const pastedResponse = await remote.app.request(
      `/api/knowledge/notebooks/${notebook.id}/sources`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: "pasted_text", text: "手动输入的证据", displayName: "手记" }),
      },
    );
    expect(pastedResponse.status).toBe(201);
    expect(await pastedResponse.json()).toMatchObject({
      source: { sourceType: "pasted_text", displayName: "手记" },
      parseArtifact: { status: "ready" },
    });

    const webResponse = await remote.app.request(
      `/api/knowledge/notebooks/${notebook.id}/sources`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: "web_snapshot", url: "https://example.com/report?token=secret" }),
      },
    );
    expect(webResponse.status).toBe(201);
    const webSource = await webResponse.json() as any;
    expect(webSource).toMatchObject({
      source: {
        sourceType: "web_snapshot",
        originMetadata: {
          kind: "web_snapshot",
          url: "https://example.com/report",
          fetchedAt: "2026-08-25T12:00:00.000Z",
        },
      },
      parseArtifact: { status: "ready" },
    });
    expect(JSON.stringify(webSource)).not.toContain("secret");
    expect(fetchWebSnapshot).toHaveBeenCalledTimes(1);

    const frozenContent = await remote.app.request(
      `/api/knowledge/snapshots/${webSource.snapshot.id}/content`,
    );
    expect(frozenContent.headers.get("content-type")).toContain("text/plain");
    expect(frozenContent.headers.get("content-security-policy")).toBe("sandbox; default-src 'none'");
    expect(Buffer.from(await frozenContent.arrayBuffer())).toEqual(webBytes);

    const escapedImport = await remote.app.request(
      `/api/knowledge/notebooks/${notebook.id}/sources`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: "pasted_text", text: "内容", filePath: inputPath }),
      },
    );
    expect(escapedImport.status).toBe(400);
    expect(await escapedImport.json()).toMatchObject({ error: "KNOWLEDGE_INVALID_ARGUMENT" });

    const wrongStudio = appHarness(localOwner("studio-b"));
    const response = await wrongStudio.app.request("/api/knowledge/notebooks");
    expect(response.status).toBe(403);
    expect(await response.json()).toMatchObject({ error: "KNOWLEDGE_STUDIO_MISMATCH" });
  });

  it("本机刷新只在内容变化时创建新快照；历史引用留在旧快照", async () => {
    const { app, knowledge, importsDir } = appHarness();
    const notebook = knowledge.createNotebook({ studioId: "studio-a", name: "版本" });
    const inputPath = path.join(importsDir, "version.txt");
    fs.writeFileSync(inputPath, "旧版本内容。\n", "utf8");
    const imported = await knowledge.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: inputPath,
    });
    const oldArtifact = await knowledge.parseSource({
      studioId: "studio-a",
      sourceId: imported.source.id,
    });
    const oldBlock = knowledge.listArtifactBlocks({
      studioId: "studio-a",
      parseArtifactId: oldArtifact.id,
    })[0];
    const historicalCitation = knowledge.createCitation({
      studioId: "studio-a",
      parseArtifactId: oldArtifact.id,
      blockId: oldBlock.id,
      startOffset: 0,
      endOffset: 3,
    });

    fs.writeFileSync(inputPath, "新版本内容。\n", "utf8");
    const refreshResponse = await app.request(
      `/api/knowledge/notebooks/${notebook.id}/sources/${imported.source.id}/refresh`,
      { method: "POST" },
    );
    expect(refreshResponse.status).toBe(200);
    const refreshed = await refreshResponse.json() as any;
    expect(refreshed).toMatchObject({ changed: true, parseArtifact: { status: "ready" } });
    expect(refreshed.snapshot.id).not.toBe(imported.snapshot.id);
    expect(JSON.stringify(refreshed)).not.toContain(inputPath);
    expect(knowledge.store.countContentSnapshots({
      studioId: "studio-a",
      sourceId: imported.source.id,
    })).toBe(2);

    const unchanged = await app.request(
      `/api/knowledge/notebooks/${notebook.id}/sources/${imported.source.id}/refresh`,
      { method: "POST" },
    );
    expect(await unchanged.json()).toMatchObject({ changed: false, snapshot: { id: refreshed.snapshot.id } });
    expect(knowledge.store.countContentSnapshots({
      studioId: "studio-a",
      sourceId: imported.source.id,
    })).toBe(2);

    const historical = await app.request(`/api/knowledge/citations/${historicalCitation.id}`);
    expect(await historical.json()).toMatchObject({
      citation: { canonicalText: "旧版本" },
      snapshot: { id: imported.snapshot.id },
    });
  });

  it("笔记本设置端点：部分更新、范围与模型引用校验、配置变更触发全量重建", async () => {
    const { app, knowledge, importsDir } = appHarness();
    const notebook = knowledge.createNotebook({ studioId: "studio-a", name: "设定" });
    const inputPath = path.join(importsDir, "notes.txt");
    fs.writeFileSync(inputPath, "第一段。\n\n第二段。\n", "utf8");
    const imported = await knowledge.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: inputPath,
    });
    await knowledge.parseSource({ studioId: "studio-a", sourceId: imported.source.id });

    const settingsUrl = `/api/knowledge/notebooks/${notebook.id}/settings`;
    const put = (body: unknown) => app.request(settingsUrl, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    expect((await put({})).status).toBe(400);
    expect((await put({ unknownKey: 1 })).status).toBe(400);
    const tooSmall = await put({ chunkTargetChars: 50 });
    expect(tooSmall.status).toBe(400);
    expect(await tooSmall.json()).toMatchObject({ error: "KNOWLEDGE_INVALID_ARGUMENT" });
    const incompleteRef = await put({ embeddingModelRef: { id: "embed-1" } });
    expect(incompleteRef.status).toBe(400);
    expect(await incompleteRef.json()).toMatchObject({ error: "KNOWLEDGE_INVALID_ARGUMENT" });
    expect((await put({ retrievalTopK: 0 })).status).toBe(400);

    // 只影响查询时行为的字段：更新成功但不触发重建（无任何摄入 job）
    // （schema 默认：新笔记本 chunkTargetChars=1200 / retrievalTopK=12）
    const queryOnly = await put({ retrievalTopK: 8 });
    expect(queryOnly.status).toBe(200);
    expect(await queryOnly.json()).toMatchObject({
      config: { retrievalTopK: 8, chunkTargetChars: null, embeddingModelRef: null },
    });
    expect(knowledge.listIngestionJobs({ studioId: "studio-a", notebookId: notebook.id })).toHaveLength(0);

    // chunkTargetChars 已随自动分块退役：PUT 显式拒绝（遗留显式列值仍生效但不再接受写入）
    const chunkUpdate = await put({ chunkTargetChars: 800 });
    expect(chunkUpdate.status).toBe(400);
    expect(await chunkUpdate.json()).toMatchObject({ error: "KNOWLEDGE_INVALID_ARGUMENT" });
    expect(knowledge.listIngestionJobs({ studioId: "studio-a", notebookId: notebook.id })).toHaveLength(0);

    // null 清除回 NULL（未配置/无上限召回）
    const cleared = await put({ retrievalTopK: null });
    expect(await cleared.json()).toMatchObject({
      config: { chunkTargetChars: null, retrievalTopK: null },
    });

    const missing = await app.request("/api/knowledge/notebooks/nb-missing/settings", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ retrievalTopK: 8 }),
    });
    expect(missing.status).toBe(404);
  });

  it("GET notebooks 带配置与摄入就绪汇总；GET ingestion 支持按笔记本/源过滤", async () => {
    const { app, knowledge, importsDir } = appHarness();
    const notebook = knowledge.createNotebook({ studioId: "studio-a", name: "状态" });
    const inputPath = path.join(importsDir, "state.txt");
    fs.writeFileSync(inputPath, "状态内容。\n", "utf8");
    const imported = await knowledge.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: inputPath,
    });
    const artifact = await knowledge.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    knowledge.enqueueSourceIngestion({
      studioId: "studio-a",
      notebookId: notebook.id,
      sourceId: imported.source.id,
      artifactId: artifact.id,
    });

    const listResponse = await app.request("/api/knowledge/notebooks");
    expect(listResponse.status).toBe(200);
    const { notebooks } = await listResponse.json() as any;
    expect(notebooks).toHaveLength(1);
    expect(notebooks[0]).toMatchObject({
      id: notebook.id,
      config: {
        embeddingModelRef: null,
        rerankModelRef: null,
        chunkTargetChars: null,
        retrievalTopK: null,
      },
      chunkTargetCharsEffective: 6553,
      sourceCount: 1,
      ingestion: { done: 0, pendingEmbedding: 0, processing: 1, failed: 0, untracked: 0 },
    });

    const ingestionResponse = await app.request(
      `/api/knowledge/ingestion?notebookId=${notebook.id}`,
    );
    expect(ingestionResponse.status).toBe(200);
    const ingestion = await ingestionResponse.json() as any;
    expect(ingestion.jobs).toHaveLength(1);
    expect(ingestion.jobs[0]).toMatchObject({
      notebookId: notebook.id,
      sourceId: imported.source.id,
      status: "queued",
      phase: "parse",
    });
    expect(ingestion.counts).toMatchObject({ queued: 1, running: 0, failed: 0, done: 0 });

    const bySource = await app.request(
      `/api/knowledge/ingestion?notebookId=${notebook.id}&sourceId=${imported.source.id}`,
    );
    expect((await bySource.json() as any).jobs).toHaveLength(1);
    const emptyNotebook = await app.request("/api/knowledge/ingestion?notebookId=nb-none");
    expect(emptyNotebook.status).toBe(200);
    expect(await emptyNotebook.json()).toMatchObject({
      jobs: [],
      counts: { queued: 0, running: 0, pending_embedding: 0, failed: 0, done: 0 },
    });

    // 最新 job 转 failed 后，就绪汇总归类到 failed
    const claimed = knowledge.store.claimNextIngestionJob();
    knowledge.store.failIngestionJob({
      studioId: "studio-a",
      jobId: claimed.id,
      error: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE: boom",
    });
    const afterFail = await app.request("/api/knowledge/notebooks");
    expect((await afterFail.json() as any).notebooks[0].ingestion).toMatchObject({
      processing: 0,
      failed: 1,
    });
  });

  it("chunks 端点：ready artifact 返回分块卡片；跨 studio 404；未 ready 422", async () => {
    const { app, knowledge, importsDir } = appHarness();
    const notebook = knowledge.createNotebook({ studioId: "studio-a", name: "分块" });
    const inputPath = path.join(importsDir, "chunks.md");
    fs.writeFileSync(inputPath, "# 第一章\n\n苹果交付日期是九月。\n\n## 数据\n\n预算八百万。\n", "utf-8");
    const imported = await knowledge.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: inputPath,
    });
    const artifact = await knowledge.parseSource({ studioId: "studio-a", sourceId: imported.source.id });

    const chunksResponse = await app.request(`/api/knowledge/parse-artifacts/${artifact.id}/chunks`);
    expect(chunksResponse.status).toBe(200);
    const body = await chunksResponse.json() as any;
    expect(typeof body.chunkerConfigId).toBe("string");
    expect(body.chunkerConfigId).toMatch(/^[0-9a-f]{16}$/);
    expect(body.chunks.length).toBeGreaterThan(0);
    // 冻结契约：ordinal 为 1-based 连续展示序号，charCount 与文本长度一致。
    body.chunks.forEach((chunk: any, index: number) => {
      expect(chunk.ordinal).toBe(index + 1);
      expect(chunk.charCount).toBe(chunk.text.length);
      expect(chunk.tokenCount).toBeGreaterThan(0);
      expect(typeof chunk.id).toBe("string");
    });
    // markdown 源的首 chunk 带 headingPath 定位（来自 block locator 索引）。
    expect(body.chunks[0].headingPath).toEqual(["第一章"]);

    // 幂等：重复请求返回同一份 chunk 数据（fingerprint 命中不重建）。
    const again = await app.request(`/api/knowledge/parse-artifacts/${artifact.id}/chunks`);
    expect(await again.json()).toEqual(body);

    // 跨 studio：studio-b 的 artifact 用 studio-a 身份访问 → 404（不泄漏存在性）。
    const otherNotebook = knowledge.createNotebook({ studioId: "studio-b", name: "外域" });
    const other = await knowledge.importPastedText({
      studioId: "studio-b",
      notebookId: otherNotebook.id,
      text: "别处内容",
      displayName: "外域.txt",
    });
    const otherArtifact = await knowledge.parseSource({ studioId: "studio-b", sourceId: other.source.id });
    const cross = await app.request(`/api/knowledge/parse-artifacts/${otherArtifact.id}/chunks`);
    expect(cross.status).toBe(404);
    expect(await cross.json()).toMatchObject({ error: "KNOWLEDGE_NOT_FOUND" });

    // 未 ready（parsing 态）artifact：显式 422，不返回半成品分块。
    const parsing = knowledge.store.beginParseArtifact({
      studioId: "studio-a",
      contentSnapshotId: imported.snapshot.id,
      parserId: "other-parser",
      parserVersion: "1",
      parserConfigHash: "d".repeat(64),
    });
    const notReady = await app.request(`/api/knowledge/parse-artifacts/${parsing.id}/chunks`);
    expect(notReady.status).toBe(422);
    expect(await notReady.json()).toMatchObject({ error: "KNOWLEDGE_PARSE_NOT_READY" });
  });

  it("GET notebooks 就绪汇总按笔记本归类：一源多笔记本不串记", async () => {
    const { app, knowledge, importsDir } = appHarness();
    const nbA = knowledge.createNotebook({ studioId: "studio-a", name: "甲" });
    const nbB = knowledge.createNotebook({ studioId: "studio-a", name: "乙" });
    const inputPath = path.join(importsDir, "shared.txt");
    fs.writeFileSync(inputPath, "共享内容。\n", "utf-8");
    const imported = await knowledge.importFile({
      studioId: "studio-a",
      notebookId: nbA.id,
      filePath: inputPath,
    });
    knowledge.addSourceToNotebook({ studioId: "studio-a", notebookId: nbB.id, sourceId: imported.source.id });
    const artifact = await knowledge.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    // 仅在乙笔记本入队摄入：甲笔记本的汇总应保持 untracked，不串到乙的 job。
    knowledge.enqueueSourceIngestion({
      studioId: "studio-a",
      notebookId: nbB.id,
      sourceId: imported.source.id,
      artifactId: artifact.id,
    });

    const listResponse = await app.request("/api/knowledge/notebooks");
    expect(listResponse.status).toBe(200);
    const { notebooks } = await listResponse.json() as any;
    const byName = new Map<string, any>(notebooks.map((nb: any) => [nb.name, nb] as [string, any]));
    expect(byName.get("甲").ingestion).toEqual({
      done: 0, pendingEmbedding: 0, processing: 0, failed: 0, untracked: 1,
    });
    expect(byName.get("乙").ingestion).toEqual({
      done: 0, pendingEmbedding: 0, processing: 1, failed: 0, untracked: 0,
    });
    expect(byName.get("甲").sourceCount).toBe(1);
    expect(byName.get("乙").sourceCount).toBe(1);
  });

  it("reingest 端点：failed 手动重试、非 failed 冲突、无 job 兜底入队、跨笔记本拒绝", async () => {
    const { app, knowledge, importsDir } = appHarness();
    const notebook = knowledge.createNotebook({ studioId: "studio-a", name: "重试" });
    const otherNotebook = knowledge.createNotebook({ studioId: "studio-a", name: "其他" });
    const inputPath = path.join(importsDir, "retry.txt");
    fs.writeFileSync(inputPath, "待重试内容。\n", "utf8");
    const imported = await knowledge.importFile({
      studioId: "studio-a",
      notebookId: notebook.id,
      filePath: inputPath,
    });
    await knowledge.parseSource({ studioId: "studio-a", sourceId: imported.source.id });
    const reingestUrl = `/api/knowledge/notebooks/${notebook.id}/sources/${imported.source.id}/reingest`;

    // 从未入队的源：兜底 enqueue
    const fallback = await app.request(reingestUrl, { method: "POST" });
    expect(fallback.status).toBe(200);
    const fallbackBody = await fallback.json() as any;
    expect(fallbackBody.retried).toBe(false);
    expect(fallbackBody.job).toMatchObject({
      notebookId: notebook.id,
      sourceId: imported.source.id,
      status: "queued",
    });

    // 最新 job 非 failed：409 冲突
    const conflict = await app.request(reingestUrl, { method: "POST" });
    expect(conflict.status).toBe(409);
    expect(await conflict.json()).toMatchObject({ error: "KNOWLEDGE_CONFLICT" });

    // 失败后手动重试：attempt 归零、回到 queued
    const claimed = knowledge.store.claimNextIngestionJob();
    knowledge.store.failIngestionJob({ studioId: "studio-a", jobId: claimed.id, error: "boom" });
    const retried = await app.request(reingestUrl, { method: "POST" });
    expect(retried.status).toBe(200);
    const retriedBody = await retried.json() as any;
    expect(retriedBody.retried).toBe(true);
    expect(retriedBody.job).toMatchObject({ id: claimed.id, status: "queued", attempt: 0 });

    // 源不在该笔记本：兜底入队经 membership 校验抛 NOT_FOUND
    const wrongNotebook = await app.request(
      `/api/knowledge/notebooks/${otherNotebook.id}/sources/${imported.source.id}/reingest`,
      { method: "POST" },
    );
    expect(wrongNotebook.status).toBe(404);
    expect(await wrongNotebook.json()).toMatchObject({ error: "KNOWLEDGE_NOT_FOUND" });
  });

  it("路由策略显式保持 Studio Owner，不依赖未知 API 兜底", () => {
    const paths = [
      ["GET", "/api/knowledge/notebooks"],
      ["POST", "/api/knowledge/notebooks"],
      ["PUT", "/api/knowledge/notebooks/nb-1/settings"],
      ["GET", "/api/knowledge/ingestion"],
      ["POST", "/api/knowledge/notebooks/nb-1/sources/src-1/reingest"],
      ["GET", "/api/knowledge/citations/cite-1"],
      ["GET", "/api/knowledge/snapshots/snap-1/content"],
    ];
    for (const [method, requestPath] of paths) {
      expect(classifyHttpRoute({ method, path: requestPath })).toMatchObject({ kind: "studio_owner" });
      expect(authorizeHttpRoute({ method, path: requestPath, principal: remoteOwner() }))
        .toMatchObject({ allowed: true });
      expect(authorizeHttpRoute({
        method,
        path: requestPath,
        principal: { ...remoteOwner(), scopes: ["chat"] },
      })).toMatchObject({ allowed: false, error: "studio_owner_required" });
    }
  });
});
