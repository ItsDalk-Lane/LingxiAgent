/**
 * jsonrpc-client.ts — 手写轻量 LSP 客户端（阶段三·14）。
 *
 * JSON-RPC over stdio（Content-Length 分帧），零新依赖（照仓库手写 MCP
 * 客户端的先例）。请求-响应按 id 关联；服务器→客户端 notification 进
 * 缓冲队列（publishDiagnostics 等由上层消费）。生命周期：initialize →
 * initialized → …requests… → shutdown/exit。进程退出/破裂：pending 请求
 * 全部拒绝（err.code=LSP_EXITED），上层如实报告并允许重启。
 */
import { ChildProcess, spawn } from "node:child_process";

export interface JsonRpcClientOptions {
  command: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  /** 请求级超时（ms）。 */
  requestTimeoutMs?: number;
  onNotification?: (method: string, params: any) => void;
  log?: { warn?: (msg: string) => void };
}

export class LspExitedError extends Error {
  code = "LSP_EXITED";
  constructor(message: string) {
    super(message);
    this.name = "LspExitedError";
  }
}

export class JsonRpcClient {
  private child: ChildProcess | null = null;
  private nextId = 1;
  private buffer = Buffer.alloc(0);
  private pending = new Map<number, { resolve: (v: any) => void; reject: (e: Error) => void; timer: NodeJS.Timeout }>();
  private exited = false;
  private exitReason: string | null = null;

  private options: JsonRpcClientOptions;

  constructor(options: JsonRpcClientOptions) {
    this.options = options;
  }

  get running(): boolean {
    return !this.exited && this.child != null;
  }

  get exitInfo(): string | null {
    return this.exitReason;
  }

  async start(): Promise<void> {
    if (this.child) return;
    this.child = spawn(this.options.command, this.options.args || [], {
      cwd: this.options.cwd,
      env: { ...process.env, ...(this.options.env || {}) },
      stdio: ["pipe", "pipe", "pipe"],
    });
    const child = this.child;
    child.stdout!.on("data", (chunk: Buffer) => this.onStdout(chunk));
    child.stderr!.on("data", (chunk: Buffer) => {
      // 语言服务器日志常走 stderr；不进结果，只留痕
      void chunk;
    });
    child.on("exit", (code, signal) => {
      this.exited = true;
      this.exitReason = `server exited (code=${code ?? "?"} signal=${signal ?? "-"})`;
      for (const [, entry] of this.pending) {
        clearTimeout(entry.timer);
        entry.reject(new LspExitedError(this.exitReason!));
      }
      this.pending.clear();
    });
    child.on("error", (err) => {
      this.exited = true;
      this.exitReason = `server failed to start: ${err.message}`;
      for (const [, entry] of this.pending) {
        clearTimeout(entry.timer);
        entry.reject(new LspExitedError(this.exitReason!));
      }
      this.pending.clear();
    });
    await new Promise<void>((resolve, reject) => {
      const onSpawnErr = (err: Error) => reject(new LspExitedError(`server failed to start: ${err.message}`));
      child.once("error", onSpawnErr);
      child.once("spawn", () => {
        child.off("error", onSpawnErr);
        resolve();
      });
    });
  }

  private onStdout(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const header = this.buffer.subarray(0, headerEnd).toString("utf8");
      const m = /Content-Length:\s*(\d+)/i.exec(header);
      if (!m) {
        // 坏帧：丢弃头部，尽力续流
        this.buffer = this.buffer.subarray(headerEnd + 4);
        continue;
      }
      const length = Number(m[1]);
      const bodyStart = headerEnd + 4;
      if (this.buffer.length < bodyStart + length) return; // 半帧等待
      const body = this.buffer.subarray(bodyStart, bodyStart + length).toString("utf8");
      this.buffer = this.buffer.subarray(bodyStart + length);
      this.handleMessage(body);
    }
  }

  private handleMessage(body: string): void {
    let msg: any;
    try {
      msg = JSON.parse(body);
    } catch {
      return;
    }
    if (msg && typeof msg === "object" && msg.method && msg.id != null) {
      // server→client request（workspace/configuration 等）：统一空响应，
      // 满足主流服务器握手；有更精细需求时再扩展。
      this.writeMessage({ jsonrpc: "2.0", id: msg.id, result: null });
      return;
    }
    if (msg && typeof msg === "object" && msg.method) {
      this.options.onNotification?.(msg.method, msg.params);
      return;
    }
    if (msg && typeof msg === "object" && msg.id != null) {
      const entry = this.pending.get(msg.id);
      if (entry) {
        this.pending.delete(msg.id);
        clearTimeout(entry.timer);
        if (msg.error) entry.reject(new Error(`${msg.error.code ?? ""} ${msg.error.message ?? "lsp error"}`.trim()));
        else entry.resolve(msg.result);
      }
    }
  }

  private writeMessage(msg: any): void {
    if (!this.child?.stdin?.writable) return;
    const body = JSON.stringify(msg);
    this.child.stdin.write(`Content-Length: ${Buffer.byteLength(body, "utf8")}\r\n\r\n${body}`);
  }

  request(method: string, params?: any): Promise<any> {
    if (this.exited || !this.child) {
      return Promise.reject(new LspExitedError(this.exitReason || "server not started"));
    }
    const id = this.nextId++;
    const timeoutMs = this.options.requestTimeoutMs ?? 15_000;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`LSP request ${method} timed out after ${timeoutMs}ms (server may still be indexing a large project — retry shortly)`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.writeMessage({ jsonrpc: "2.0", id, method, params: params ?? null });
    });
  }

  notify(method: string, params?: any): void {
    if (this.exited) return;
    this.writeMessage({ jsonrpc: "2.0", method, params: params ?? null });
  }

  async initialize(rootUri: string, capabilities: any = {}): Promise<any> {
    const result = await this.request("initialize", {
      processId: process.pid,
      rootUri,
      capabilities,
    });
    this.notify("initialized", {});
    return result;
  }

  async shutdown(): Promise<void> {
    if (!this.child) return;
    try {
      await this.request("shutdown");
    } catch { /* 尽力 */ }
    try {
      this.notify("exit");
    } catch { /* 同上 */ }
    const child = this.child;
    this.child = null;
    setTimeout(() => {
      try { child.kill(); } catch { /* 尽力 */ }
    }, 300).unref?.();
  }
}
