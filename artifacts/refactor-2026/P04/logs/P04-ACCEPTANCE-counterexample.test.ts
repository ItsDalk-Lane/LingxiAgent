import { describe, expect, it } from "vitest";
import http from "node:http";
import { AddressInfo } from "node:net";
import { EmbeddingClient } from "../../../../core/model-operation-client.ts";

// 独立验收反例：真实 globalThis.fetch + 真实 loopback HTTP server（不用 mock fetch）。
async function startCountingServer(mode: "ok" | "hang") {
  let hits = 0;
  const sockets = new Set<any>();
  const server = http.createServer((req, res) => {
    hits += 1;
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      if (mode === "hang") {
        // 写完头后挂住不结束（断流）
        res.writeHead(200, { "content-type": "application/json" });
        res.write("{\"data\":[");
        return;
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ data: [{ index: 0, embedding: [1, 0, 0] }], usage: { total_tokens: 3 } }));
    });
  });
  server.on("connection", (s) => { sockets.add(s); s.on("close", () => sockets.delete(s)); });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  return {
    url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`,
    hits: () => hits,
    openSockets: () => sockets.size,
    close: () => new Promise<void>((r) => server.close(() => r())),
  };
}

function ledger() {
  const calls: any[] = [];
  return {
    calls,
    start: () => ({ requestId: "usage-x" }),
    finish: (...a: any[]) => calls.push(["finish", ...a]),
    recordError: (...a: any[]) => calls.push(["recordError", ...a]),
  };
}

const execution = (baseUrl: string) => ({
  operation: "embedding",
  provider: "provider-a",
  api: "openai-embeddings",
  apiKey: "acceptance-key",
  baseUrl,
  model: { id: "embed-model", provider: "provider-a", dimensions: 3 },
});

describe("P04 独立验收反例（真实 fetch + 真实 server）", () => {
  it("迟到取消：刷新期间 abort，刷新完成后真实 fetch 不发出任何请求", async () => {
    const server = await startCountingServer("ok");
    const controller = new AbortController();
    const resolveCalls: string[] = [];
    const client = new EmbeddingClient({
      resolveOperationFresh: async () => {
        await new Promise((r) => setTimeout(r, 25));
        controller.abort(); // 凭证仍在等待中时任务被取消，随后刷新完成
        resolveCalls.push("resolved");
        return execution(server.url);
      },
      getUsageLedger: () => ledger(), // 不传 fetch → 真实 globalThis.fetch
    });
    const err = await client.embed({ texts: ["x"], signal: controller.signal }).then(
      () => null, (e) => e,
    );
    expect(err?.name).toBe("AbortError");
    expect(resolveCalls).toEqual(["resolved"]); // 刷新确实完成（凭证返回了）
    expect(server.hits()).toBe(0); // 真实服务器零请求——取消后不复活
    await server.close();
  }, 20_000);

  it("中途断流：请求已发出后 abort——单次 attempt、无自动重试复活", async () => {
    const server = await startCountingServer("hang");
    const usage = ledger();
    const controller = new AbortController();
    const client = new EmbeddingClient({
      resolveOperationFresh: async () => execution(server.url),
      getUsageLedger: () => usage,
    });
    const promise = client.embed({ texts: ["x"], signal: controller.signal });
    setTimeout(() => controller.abort(), 120);
    const err = await promise.then(() => null, (e) => e);
    expect(err?.name).toBe("AbortError");
    expect(server.hits()).toBe(1); // 恰一次 attempt
    expect(usage.calls.some((c) => c[0] === "recordError" && c[3] === "aborted")).toBe(true);
    await new Promise((r) => setTimeout(r, 150)); // 观察是否有迟到复活/重试
    expect(server.hits()).toBe(1); // 无第二次请求
    await server.close();
  }, 20_000);
});
