// mock LSP server（阶段三·14 测试夹具）：Content-Length 帧的极简语言服务器。
// 应答：initialize / textDocument/definition / references / hover / documentSymbol /
// prepareRename / rename；didOpen 后推送一次 publishDiagnostics。
// 行为开关（环境变量 MOCK_LSP_*）：定义位置、诊断条数、rename 编辑、
// 是否超时（测超时文案）、是否直接退出（测 LSP_EXITED）。
import { stdin, stdout } from "node:process";

let buffer = Buffer.alloc(0);
let nextId = 1;
const pending = [];

function send(msg) {
  const body = JSON.stringify(msg);
  stdout.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}

function handle(msg) {
  if (msg.method && msg.id != null) {
    const id = msg.id;
    if (msg.method === "initialize") {
      send({ jsonrpc: "2.0", id, result: { capabilities: { renameProvider: { prepareProvider: true }, hoverProvider: true, definitionProvider: true, referencesProvider: true, documentSymbolProvider: true, diagnosticProvider: { interFileDependencies: false, workspaceDiagnostics: false } } } });
      return;
    }
    if (msg.method === "shutdown") {
      send({ jsonrpc: "2.0", id, result: null });
      return;
    }
    if (msg.method === "textDocument/definition") {
      send({ jsonrpc: "2.0", id, result: [{ uri: "file:///workspace/target.ts", range: { start: { line: 4, character: 0 }, end: { line: 4, character: 9 } } }] });
      return;
    }
    if (msg.method === "textDocument/references") {
      send({ jsonrpc: "2.0", id, result: [
        { uri: "file:///workspace/a.ts", range: { start: { line: 2, character: 8 } } },
        { uri: "file:///workspace/b.ts", range: { start: { line: 9, character: 4 } } },
      ] });
      return;
    }
    if (msg.method === "textDocument/hover") {
      send({ jsonrpc: "2.0", id, result: { contents: { kind: "markdown", value: "```ts\nfunction greet(name: string): string\n```" } } });
      return;
    }
    if (msg.method === "textDocument/documentSymbol") {
      send({ jsonrpc: "2.0", id, result: [
        { name: "greet", kind: 12, range: { start: { line: 3 } }, children: [] },
        { name: "main", kind: 12, range: { start: { line: 8 } }, children: [
          { name: "inner", kind: 13, range: { start: { line: 10 } }, children: [] },
        ] },
      ] });
      return;
    }
    if (msg.method === "textDocument/prepareRename") {
      if (process.env.MOCK_LSP_RENAME_REJECT === "1") {
        send({ jsonrpc: "2.0", id, error: { code: -32600, message: "cannot rename this symbol" } });
        return;
      }
      send({ jsonrpc: "2.0", id, result: { range: { start: { line: 0, character: 9 }, end: { line: 0, character: 14 } } } });
      return;
    }
    if (msg.method === "textDocument/rename") {
      const uri = msg.params?.textDocument?.uri || "file:///workspace/a.ts";
      send({ jsonrpc: "2.0", id, result: { changes: { [uri]: [
        { range: { start: { line: 0, character: 9 }, end: { line: 0, character: 14 } }, newText: msg.params?.newName || "renamed" },
      ] } } });
      return;
    }
    if (msg.method.startsWith("textDocument/")) {
      send({ jsonrpc: "2.0", id, result: null });
      return;
    }
    send({ jsonrpc: "2.0", id, result: null });
    return;
  }
  if (msg.method === "initialized") return;
  if (msg.method === "textDocument/didOpen") {
    send({ jsonrpc: "2.0", method: "textDocument/publishDiagnostics", params: { uri: msg.params?.textDocument?.uri, diagnostics: [
      { range: { start: { line: 2, character: 0 } }, severity: 1, message: "mock error: something is off" },
      { range: { start: { line: 5, character: 3 } }, severity: 2, message: "mock warning: unused variable" },
    ] } });
    return;
  }
}

if (process.env.MOCK_LSP_EXIT_EARLY === "1") {
  process.exit(3);
}

stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    const headerEnd = buffer.indexOf("\r\n\r\n");
    if (headerEnd < 0) return;
    const m = /Content-Length:\s*(\d+)/i.exec(buffer.subarray(0, headerEnd).toString("utf8"));
    if (!m) { buffer = buffer.subarray(headerEnd + 4); continue; }
    const length = Number(m[1]);
    const bodyStart = headerEnd + 4;
    if (buffer.length < bodyStart + length) return;
    const body = buffer.subarray(bodyStart, bodyStart + length).toString("utf8");
    buffer = buffer.subarray(bodyStart + length);
    try { handle(JSON.parse(body)); } catch { /* 忽略坏帧 */ }
  }
});
void nextId; void pending;
