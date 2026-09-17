/**
 * lsp-tool.ts — 单工具多动作的语言服务桥（阶段三·14）。
 *
 * 手写 JSON-RPC 客户端（lib/lsp/jsonrpc-client.ts，零新依赖）。注册表：
 * typescript(typescript-language-server) / python(pyright-langserver) /
 * go(gopls) / rust(rust-analyzer) / c/c++(clangd)。按 (sessionPath, language)
 * 懒启动 + 空闲停泊（IDLE_STOP_MS 无调用即 shutdown）。7 动作：
 * status / diagnostics / definition / references / hover / symbols / rename。
 * 读类=read 权限；rename=write（prepareRename 验证 → rename 计算 →
 * 写回经注入的 writeFile）。行号统一 1-based 输入输出。服务器缺失→
 * 报错并指路环境依赖页（该语言不注册语义由调用侧呈现）。
 */
import path from "node:path";
import { Type, StringEnum } from "../pi-sdk/index.ts";
import { JsonRpcClient, LspExitedError } from "../lsp/jsonrpc-client.ts";

/** 空闲停泊：超过该时长无调用即 shutdown（下次调用重新起+索引）。 */
export const LSP_IDLE_STOP_MS = 5 * 60_000;
const REQUEST_TIMEOUT_MS = 20_000;

export interface LspLanguageEntry {
  languageId: string;
  serverCommand: string;
  serverArgs: string[];
  /** 该语言常见扩展名（文件→语言路由用）。 */
  extensions: string[];
  /** env-deps 登记表 id（安装指引对齐）。 */
  envDepId: string;
}

export const LSP_LANGUAGES: Record<string, LspLanguageEntry> = {
  typescript: {
    languageId: "typescript",
    serverCommand: "typescript-language-server",
    serverArgs: ["--stdio"],
    extensions: [".ts", ".tsx", ".js", ".jsx"],
    envDepId: "typescript",
  },
  python: {
    languageId: "python",
    serverCommand: "pyright-langserver",
    serverArgs: ["--stdio"],
    extensions: [".py"],
    envDepId: "pyright",
  },
  go: {
    languageId: "go",
    serverCommand: "gopls",
    serverArgs: [],
    extensions: [".go"],
    envDepId: "gopls",
  },
  rust: {
    languageId: "rust",
    serverCommand: "rust-analyzer",
    serverArgs: [],
    extensions: [".rs"],
    envDepId: "rust_analyzer",
  },
  cpp: {
    languageId: "cpp",
    serverCommand: "clangd",
    serverArgs: [],
    extensions: [".c", ".h", ".cpp", ".hpp", ".cc"],
    envDepId: "clangd",
  },
};

export function lspLanguageForFile(filePath: string): LspLanguageEntry | null {
  const ext = path.extname(filePath).toLowerCase();
  for (const entry of Object.values(LSP_LANGUAGES)) {
    if (entry.extensions.includes(ext)) return entry;
  }
  return null;
}

interface ManagedServer {
  client: JsonRpcClient;
  language: LspLanguageEntry;
  opened: Set<string>;
  lastUsedAt: number;
  /** publishDiagnostics 缓冲：uri → 最新诊断数组。 */
  diagnostics: Map<string, any[]>;
  stopTimer: NodeJS.Timeout | null;
}

export interface LspToolDeps {
  cwd: string;
  getSessionPath: () => string | null;
  /** 服务器可用性探测（缺省 spawn --version；测试注入）。 */
  probeServer?: (entry: LspLanguageEntry) => Promise<boolean>;
  /** rename 写回（沙盒资源 IO 通道；缺省拒绝应用）。 */
  writeFile?: (absolutePath: string, content: string) => Promise<void>;
  readFile?: (absolutePath: string) => Promise<string>;
  now?: () => number;
  log?: { warn?: (msg: string) => void };
}

import { spawnSync } from "node:child_process";

async function defaultProbeServer(entry: LspLanguageEntry): Promise<boolean> {
  try {
    const result = spawnSync(entry.serverCommand, ["--version"], { timeout: 10_000, stdio: "ignore" });
    return !result.error && result.status === 0;
  } catch {
    return false;
  }
}

function fileUri(absolutePath: string): string {
  return `file://${absolutePath}`;
}

/** LSP Location[]/LocationLink[] → 1-based 摘要行。 */
export function summarizeLocations(locations: any): Array<{ file: string; line: number; column: number }> {
  const arr = Array.isArray(locations) ? locations : locations ? [locations] : [];
  const out: Array<{ file: string; line: number; column: number }> = [];
  for (const loc of arr) {
    const target = loc?.targetRange?.start ?? loc?.range?.start ?? loc?.targetSelectionRange?.start;
    const uri = loc?.targetUri ?? loc?.uri;
    if (!uri || !target) continue;
    out.push({
      file: decodeURIComponent(uri.replace(/^file:\/\//, "")),
      line: (target.line ?? 0) + 1,
      column: (target.character ?? 0) + 1,
    });
  }
  return out;
}

function summarizeDiagnostics(diagnostics: any[]): Array<{ severity: string; line: number; message: string }> {
  const names = ["?", "error", "warning", "info", "hint"];
  return diagnostics.slice(0, 100).map((d: any) => ({
    severity: names[d.severity ?? 0] || "info",
    line: (d.range?.start?.line ?? 0) + 1,
    message: String(d.message ?? "").split("\n")[0].slice(0, 300),
  }));
}

export function createLspTool(deps: LspToolDeps) {
  const now = deps.now || (() => Date.now());
  const probeServer = deps.probeServer || defaultProbeServer;
  /** (sessionKey, languageId) → ManagedServer。 */
  const servers = new Map<string, ManagedServer>();

  function sessionKey(): string {
    return deps.getSessionPath?.() || deps.cwd;
  }

  function armIdleStop(managed: ManagedServer): void {
    if (managed.stopTimer) clearTimeout(managed.stopTimer);
    managed.stopTimer = setTimeout(() => {
      void managed.client.shutdown().catch(() => { /* 尽力 */ });
      for (const [key, value] of servers) {
        if (value === managed) servers.delete(key);
      }
    }, LSP_IDLE_STOP_MS);
    managed.stopTimer.unref?.();
  }

  async function ensureServer(language: LspLanguageEntry): Promise<ManagedServer> {
    const key = `${sessionKey()}::${language.languageId}`;
    const existing = servers.get(key);
    if (existing && existing.client.running) {
      existing.lastUsedAt = now();
      armIdleStop(existing);
      return existing;
    }
    if (existing) servers.delete(key);

    const diagnosticsBuffer = new Map<string, any[]>();
    const client = new JsonRpcClient({
      command: language.serverCommand,
      args: language.serverArgs,
      cwd: deps.cwd,
      requestTimeoutMs: REQUEST_TIMEOUT_MS,
      onNotification: (method, params) => {
        if (method === "textDocument/publishDiagnostics" && params?.uri) {
          diagnosticsBuffer.set(params.uri, Array.isArray(params.diagnostics) ? params.diagnostics : []);
        }
      },
      log: deps.log,
    });
    await client.start();
    const root = deps.cwd.replace(/\/+$/, "");
    await client.initialize(fileUri(root), {
      textDocument: {
        synchronization: { openClose: true, change: 1 },
        rename: { prepareSupport: true },
      },
      workspace: { workspaceFolders: false },
    });
    const managed: ManagedServer = {
      client,
      language,
      opened: new Set(),
      lastUsedAt: now(),
      diagnostics: diagnosticsBuffer,
      stopTimer: null,
    };
    armIdleStop(managed);
    servers.set(key, managed);
    return managed;
  }

  async function openDocument(managed: ManagedServer, absolutePath: string, text: string, languageId: string): Promise<void> {
    const uri = fileUri(absolutePath);
    const version = managed.opened.has(uri) ? 2 : 1;
    managed.opened.add(uri);
    if (version === 1) {
      managed.client.notify("textDocument/didOpen", {
        textDocument: { uri, languageId, version, text },
      });
    } else {
      managed.client.notify("textDocument/didChange", {
        textDocument: { uri, version },
        contentChanges: [{ text }],
      });
    }
  }

  function resolveFile(filePath: string): string {
    return path.isAbsolute(filePath) ? filePath : path.resolve(deps.cwd, filePath);
  }

  return {
    name: "lsp",
    description: "Language-server powered code intelligence: go to definition, find references, hover docs, document symbols, live diagnostics, and safe rename — per language (typescript/python/go/rust/cpp). Positions are 1-based line/column. Servers start lazily per language and park when idle; the first call on a big project may be slow while indexing (the timeout message says so). A language whose server is missing reports an install hint (Env Dependencies settings page). Prefer lsp over grep for symbol navigation; rename validates via prepareRename before computing and writing edits.",
    parameters: Type.Object({
      action: StringEnum(["status", "diagnostics", "definition", "references", "hover", "symbols", "rename"], { description: "status: which languages/servers are available. diagnostics: current file diagnostics. definition/references: symbol at file:line:column. hover: type/doc at position. symbols: document outline. rename: rename symbol at position with new_name (validated first, then applied)" }),
      language: Type.String({ description: "typescript | python | go | rust | cpp" }),
      file: Type.String({ description: "File path (relative to cwd ok)" }),
      line: Type.Number({ description: "1-based line of the symbol/position" }),
      column: Type.Number({ description: "1-based column (character) of the position" }),
      new_name: Type.String({ description: "New name for action=rename" }),
    }),
    sessionPermission: {
      resolveInvocation: (input: any = {}) => {
        if (input?.action && input.action !== "rename") {
          return { action: input.action, kind: "read", capability: `lsp.${input.action}` };
        }
        if (!input?.action) {
          return { action: "status", kind: "read", capability: "lsp.status" };
        }
        return { action: "rename", kind: "write", capability: "lsp.rename" };
      },
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      const action = params?.action || "status";
      const languageId = typeof params?.language === "string" ? params.language.trim() : "";
      const language = LSP_LANGUAGES[languageId];

      if (action === "status") {
        const availability = await Promise.all(
          Object.values(LSP_LANGUAGES).map(async (entry) => ({
            language: entry.languageId,
            server: entry.serverCommand,
            available: await probeServer(entry),
          })),
        );
        const lines = [
          "lsp status:",
          ...availability.map((a) => `  ${a.language}: ${a.available ? `ready (${a.server})` : `server missing (${a.server}) — install it; see the Env Dependencies settings page`}`),
        ];
        return { content: [{ type: "text", text: lines.join("\n") }], details: { availability } };
      }

      if (!language) {
        return { isError: true, content: [{ type: "text", text: `language must be one of: ${Object.keys(LSP_LANGUAGES).join(", ")} (got "${languageId}")` }] };
      }
      if (!(await probeServer(language))) {
        return {
          isError: true,
          content: [{ type: "text", text: `${language.serverCommand} is not installed — the ${language.languageId} language service is unavailable. Install it and check the Env Dependencies settings page.` }],
          details: { errorCode: "LSP_SERVER_MISSING", server: language.serverCommand },
        };
      }
      if (action !== "diagnostics" && action !== "symbols" && !params?.file) {
        return { content: [{ type: "text", text: "file is required for this action" }] };
      }

      const filePath = params?.file ? resolveFile(String(params.file)) : null;
      const line = Math.max(1, Number(params?.line) || 1) - 1;
      const column = Math.max(1, Number(params?.column) || 1) - 1;

      let managed: ManagedServer;
      try {
        managed = await ensureServer(language);
      } catch (err: any) {
        if (err instanceof LspExitedError || err?.code === "LSP_EXITED") {
          return { isError: true, content: [{ type: "text", text: `failed to start ${language.serverCommand}: ${err.message}` }], details: { errorCode: "LSP_START_FAILED" } };
        }
        return { isError: true, content: [{ type: "text", text: `failed to start ${language.serverCommand}: ${err?.message || String(err)}` }], details: { errorCode: "LSP_START_FAILED" } };
      }

      try {
        // 单文件同步（didOpen/didChange），读类动作即问即答
        let text: string | null = null;
        if (filePath) {
          text = deps.readFile ? await deps.readFile(filePath) : null;
          if (text != null) await openDocument(managed, filePath, text, language.languageId);
        }

        if (action === "diagnostics") {
          if (!filePath) return { content: [{ type: "text", text: "file is required for diagnostics" }] };
          const uri = fileUri(filePath);
          // publishDiagnostics 是推送：给服务器一点时间发布
          await new Promise((r) => setTimeout(r, 500));
          const diags = managed.diagnostics.get(uri) || [];
          const summary = summarizeDiagnostics(diags);
          if (!summary.length) {
            return { content: [{ type: "text", text: "no diagnostics" }], details: { count: 0 } };
          }
          const lines = summary.map((d) => `[${d.severity}] ${path.relative(deps.cwd, filePath)}:${d.line} — ${d.message}`);
          return { content: [{ type: "text", text: lines.join("\n") }], details: { count: summary.length, diagnostics: summary } };
        }

        if (action === "symbols") {
          if (!filePath) return { content: [{ type: "text", text: "file is required for symbols" }] };
          const symbols = await managed.client.request("textDocument/documentSymbol", {
            textDocument: { uri: fileUri(filePath) },
          });
          const flat: Array<{ name: string; kind: number; line: number }> = [];
          const walk = (items: any[]) => {
            for (const item of items || []) {
              const start = item.range?.start ?? item.selectionRange?.start;
              flat.push({ name: item.name, kind: item.kind, line: (start?.line ?? 0) + 1 });
              if (Array.isArray(item.children)) walk(item.children);
            }
          };
          walk(Array.isArray(symbols) ? symbols : []);
          const lines = flat.slice(0, 200).map((s) => `${s.name} (kind ${s.kind}) :${s.line}`);
          return {
            content: [{ type: "text", text: lines.length ? lines.join("\n") : "no symbols" }],
            details: { count: flat.length, symbols: flat.slice(0, 200) },
          };
        }

        const position = { line, character: column };
        const textDocument = { textDocument: { uri: fileUri(filePath!) } };

        if (action === "definition" || action === "references") {
          const method = action === "definition" ? "textDocument/definition" : "textDocument/references";
          const result = await managed.client.request(method, {
            ...textDocument,
            position,
            ...(action === "references" ? { context: { includeDeclaration: true } } : {}),
          });
          const locations = summarizeLocations(result);
          if (!locations.length) return { content: [{ type: "text", text: `no ${action} found` }], details: { count: 0 } };
          const lines = locations.slice(0, 50).map((l) => `${path.relative(deps.cwd, l.file) || l.file}:${l.line}:${l.column}`);
          return { content: [{ type: "text", text: lines.join("\n") }], details: { count: locations.length, locations: locations.slice(0, 50) } };
        }

        if (action === "hover") {
          const result = await managed.client.request("textDocument/hover", { ...textDocument, position });
          const value = result?.contents?.value ?? (typeof result?.contents === "string" ? result.contents : null);
          if (!value) return { content: [{ type: "text", text: "no hover info at this position" }], details: { hover: null } };
          return { content: [{ type: "text", text: String(value).slice(0, 4000) }], details: { hover: String(value).slice(0, 2000) } };
        }

        // ── rename：先验证后写 ──
        const newName = typeof params?.new_name === "string" ? params.new_name.trim() : "";
        if (!newName) return { content: [{ type: "text", text: "new_name is required for rename" }] };
        try {
          const preparable = await managed.client.request("textDocument/prepareRename", { ...textDocument, position });
          if (!preparable) {
            return { isError: true, content: [{ type: "text", text: "symbol at this position cannot be renamed (prepareRename rejected)" }], details: { errorCode: "LSP_RENAME_NOT_PREPARABLE" } };
          }
        } catch (err: any) {
          // 只有「服务器不支持该方法」才跳过验证；显式拒绝（如 cannot rename
          // this symbol）= 不可重命名，如实拒绝。
          if (!/-32601|MethodNotFound|method not found/i.test(String(err?.message || ""))) {
            return { isError: true, content: [{ type: "text", text: `rename rejected by the language server: ${err?.message || err}` }], details: { errorCode: "LSP_RENAME_NOT_PREPARABLE" } };
          }
          deps.log?.warn?.(`[lsp] prepareRename unavailable: ${err?.message || err}`);
        }
        const edit = await managed.client.request("textDocument/rename", { ...textDocument, position, newName });
        const changes = edit?.changes ?? {};
        const uris = Object.keys(changes);
        if (!uris.length) {
          return { content: [{ type: "text", text: "rename returned no edits — nothing changed" }], details: { changedFiles: [] } };
        }
        if (!deps.writeFile) {
          const preview = uris.map((u) => `${u}: ${changes[u].length} edit(s)`).join("\n");
          return { content: [{ type: "text", text: `rename computed ${uris.length} file(s) of edits but writing is unavailable in this runtime:\n${preview}` }], details: { applied: false, files: uris } };
        }
        const changed: string[] = [];
        for (const uri of uris) {
          const target = decodeURIComponent(uri.replace(/^file:\/\//, ""));
          const current = deps.readFile ? await deps.readFile(target).catch(() => null) : null;
          if (current == null) continue;
          // 应用 WorkspaceEdit：按（行,列）从尾往头逐条做字符串切片替换，
          // 后面的编辑先落位，前面的偏移不受影响。
          const sorted = [...changes[uri]].sort((a: any, b: any) => (
            (b.range.start.line - a.range.start.line)
            || (b.range.start.character - a.range.start.character)
          ));
          const lines = current.split("\n");
          let ok = true;
          for (const change of sorted) {
            const sl = change.range.start.line;
            const sc = change.range.start.character;
            const el = change.range.end.line;
            const ec = change.range.end.character;
            if (sl >= lines.length || el >= lines.length || sl < 0 || el < sl) { ok = false; break; }
            if (sl === el) {
              lines[sl] = lines[sl].slice(0, sc) + (change.newText ?? "") + lines[el].slice(ec);
            } else {
              const head = lines[sl].slice(0, sc) + (change.newText ?? "") + lines[el].slice(ec);
              lines.splice(sl, el - sl + 1, head);
            }
          }
          if (!ok) continue;
          const updated = lines.join("\n");
          await deps.writeFile(target, updated);
          changed.push(path.relative(deps.cwd, target) || target);
          managed.opened.delete(uri);
        }
        return {
          content: [{ type: "text", text: `renamed to "${newName}" — ${changed.length} file(s) updated:\n${changed.map((f) => `- ${f}`).join("\n")}` }],
          details: { applied: true, changedFiles: changed },
        };
      } catch (err: any) {
        if (err instanceof LspExitedError || err?.code === "LSP_EXITED") {
          return { isError: true, content: [{ type: "text", text: `language server ${language.serverCommand} died mid-request: ${err.message}` }], details: { errorCode: "LSP_EXITED" } };
        }
        return { isError: true, content: [{ type: "text", text: `${action} failed: ${err?.message || String(err)}` }], details: { errorCode: "LSP_REQUEST_FAILED" } };
      }
    },
  };
}
