import { afterEach, describe, expect, it } from "vitest";
import http from "http";
import path from "path";
import { spawn, type ChildProcessWithoutNullStreams } from "child_process";
import { WebSocketServer } from "ws";

const children: ChildProcessWithoutNullStreams[] = [];
const servers: http.Server[] = [];

afterEach(async () => {
  for (const child of children.splice(0)) {
    if (child.exitCode === null) child.kill("SIGKILL");
  }
  await Promise.all(servers.splice(0).map((server) => new Promise<void>((resolve) => {
    if (!server.listening) return resolve();
    server.close(() => resolve());
  })));
});

describe("CLI chat real WebSocket boundary", () => {
  it("hides another session's stream frame and exits nonzero on an unexpected disconnect", async () => {
    const selectedPath = "/synthetic/session-one.jsonl";
    const server = http.createServer((request, response) => {
      response.setHeader("Content-Type", "application/json");
      if (request.url === "/api/health") response.end(JSON.stringify({ agent: "Hana", agentId: "a1", version: "test" }));
      else if (request.url === "/api/agents") response.end(JSON.stringify({ agents: [] }));
      else if (request.url === "/api/sessions") response.end(JSON.stringify([
        { sessionId: "s1", path: selectedPath, title: "Selected" },
      ]));
      else if (request.url === "/api/sessions/switch") response.end(JSON.stringify({ ok: true }));
      else { response.statusCode = 404; response.end(JSON.stringify({ error: "not found" })); }
    });
    servers.push(server);
    const wsServer = new WebSocketServer({ noServer: true });
    server.on("upgrade", (request, socket, head) => {
      if (request.url !== "/ws" || request.headers.authorization !== "Bearer synthetic-token") {
        socket.destroy();
        return;
      }
      wsServer.handleUpgrade(request, socket, head, (socket) => wsServer.emit("connection", socket, request));
    });
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => {
        server.off("error", reject);
        resolve();
      });
    });
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("unexpected test listener address");

    const child = spawn(process.execPath, [
      path.join(process.cwd(), "cli", "entry.ts"), "chat", "--plain",
      "--url", `http://127.0.0.1:${address.port}`, "--token", "synthetic-token", "--session", selectedPath,
    ], { cwd: process.cwd(), stdio: ["pipe", "pipe", "pipe"] });
    children.push(child);
    let stdout = "";
    let stderr = "";
    let prompt: any = null;
    child.stdout.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
    child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
    wsServer.on("connection", (socket) => {
      setTimeout(() => child.stdin.write("hello\n"), 20);
      socket.on("message", (data) => {
        const input = JSON.parse(data.toString());
        if (input.type !== "prompt") return;
        prompt = input;
        socket.send(JSON.stringify({ type: "text_delta", sessionId: "s2", sessionPath: "/synthetic/other.jsonl", streamId: "wrong", delta: "WRONG_SESSION_TEXT" }));
        socket.send(JSON.stringify({ type: "text_delta", sessionId: "s1", sessionPath: selectedPath, streamId: "current", delta: "RIGHT_SESSION_TEXT" }));
        setTimeout(() => socket.close(), 20);
      });
    });

    let timeout: NodeJS.Timeout;
    const exitCode = await Promise.race([
      new Promise<number | null>((resolve) => child.once("exit", (code) => resolve(code))),
      new Promise<never>((_resolve, reject) => {
        timeout = setTimeout(() => reject(new Error("CLI did not stop after disconnect")), 5000);
      }),
    ]).finally(() => clearTimeout(timeout));
    expect(exitCode).toBe(1);
    expect(prompt).toMatchObject({ sessionId: "s1", sessionPath: selectedPath, text: "hello" });
    expect(stdout).toContain("RIGHT_SESSION_TEXT");
    expect(stdout).not.toContain("WRONG_SESSION_TEXT");
    expect(stderr).toBe("");
    wsServer.close();
  });
});
