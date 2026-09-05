import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

export const EXACT_BOUNDARY_ALLOWLISTS = Object.freeze({
  mcpCallTool: Object.freeze([
    "core/mcp/clients/http-client.ts",
    "core/mcp/manager.ts",
  ]),
  pluginExecuteTool: Object.freeze(["core/plugin-dev-service.ts"]),
  canonicalTargetExecutor: Object.freeze(["core/tool-invocation-gateway.ts"]),
});

const SOURCE_ROOTS = Object.freeze([
  "cli",
  "core",
  "desktop",
  "hub",
  "lib",
  "packages",
  "plugins",
  "server",
  "shared",
  "tools",
]);
const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"]);
const BRIDGE_FORBIDDEN_IDENTIFIERS = new Set([
  "builtinCall",
  "mcpCall",
  "resolveBuiltinInvocation",
]);
const ENGINE_DEFERRED_RAW_MAP_IDENTIFIERS = new Set([
  "builtinToolsByName",
  "deferredBuiltinToolsByName",
  "deferredRawToolsByName",
  "deferredToolObjects",
]);

function toPosixRelative(rootDir, filename) {
  return path.relative(rootDir, filename).split(path.sep).join("/");
}

function collectSourceFiles(rootDir) {
  const files = [];
  const visit = (directory) => {
    if (!fs.existsSync(directory)) return;
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolute);
      } else if (entry.isFile() && SOURCE_EXTENSIONS.has(path.extname(entry.name))) {
        files.push(absolute);
      }
    }
  };
  for (const sourceRoot of SOURCE_ROOTS) visit(path.join(rootDir, sourceRoot));
  return files.sort();
}

function scriptKindFor(filename) {
  if (filename.endsWith(".tsx")) return ts.ScriptKind.TSX;
  if (filename.endsWith(".jsx")) return ts.ScriptKind.JSX;
  if (filename.endsWith(".js") || filename.endsWith(".mjs") || filename.endsWith(".cjs")) {
    return ts.ScriptKind.JS;
  }
  return ts.ScriptKind.TS;
}

function invokedMemberName(expression) {
  if (ts.isPropertyAccessExpression(expression)) return expression.name.text;
  if (ts.isElementAccessExpression(expression) && ts.isStringLiteralLike(expression.argumentExpression)) {
    return expression.argumentExpression.text;
  }
  return null;
}

function locationOf(sourceFile, node) {
  const start = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
  return { line: start.line + 1, column: start.character + 1 };
}

function addViolation(violations, sourceFile, relativePath, node, rule, message) {
  violations.push({
    file: relativePath,
    ...locationOf(sourceFile, node),
    rule,
    message,
  });
}

function scanSourceFile(rootDir, filename, violations) {
  const relativePath = toPosixRelative(rootDir, filename);
  const sourceFile = ts.createSourceFile(
    filename,
    fs.readFileSync(filename, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    scriptKindFor(filename),
  );
  const mcpAllowlist = new Set(EXACT_BOUNDARY_ALLOWLISTS.mcpCallTool);
  const pluginAllowlist = new Set(EXACT_BOUNDARY_ALLOWLISTS.pluginExecuteTool);
  const canonicalAllowlist = new Set(EXACT_BOUNDARY_ALLOWLISTS.canonicalTargetExecutor);

  const visit = (node) => {
    if (relativePath === "core/tool-catalog-bridge.ts"
      && ts.isIdentifier(node)
      && BRIDGE_FORBIDDEN_IDENTIFIERS.has(node.text)) {
      addViolation(
        violations,
        sourceFile,
        relativePath,
        node,
        "bridge-raw-adapter",
        `ToolCatalog Bridge may not reference ${node.text}.`,
      );
    }

    if (relativePath === "core/engine.ts"
      && ts.isIdentifier(node)
      && ENGINE_DEFERRED_RAW_MAP_IDENTIFIERS.has(node.text)) {
      addViolation(
        violations,
        sourceFile,
        relativePath,
        node,
        "engine-deferred-raw-map",
        `Engine may not retain deferred raw tool objects in ${node.text}.`,
      );
    }

    if (ts.isCallExpression(node)) {
      const memberName = invokedMemberName(node.expression);
      if (memberName === "callTool" && !mcpAllowlist.has(relativePath)) {
        addViolation(
          violations,
          sourceFile,
          relativePath,
          node,
          "mcp-raw-execution",
          "MCP callTool must stay inside an exact source-adapter allowlist.",
        );
      }
      if (memberName === "executePluginTool" && !pluginAllowlist.has(relativePath)) {
        addViolation(
          violations,
          sourceFile,
          relativePath,
          node,
          "plugin-raw-execution",
          "PluginManager.executePluginTool must stay inside the plugin source adapter.",
        );
      }
      if (memberName === "executeCanonical" && !canonicalAllowlist.has(relativePath)) {
        addViolation(
          violations,
          sourceFile,
          relativePath,
          node,
          "canonical-executor-bypass",
          "Registered target executors may only be invoked by ToolInvocationGateway.",
        );
      }
    }
    ts.forEachChild(node, visit);
  };

  visit(sourceFile);
}

export function scanToolInvocationBoundaries({ rootDir = process.cwd() } = {}) {
  const resolvedRoot = path.resolve(rootDir);
  const sourceFiles = collectSourceFiles(resolvedRoot);
  const violations = [];
  for (const filename of sourceFiles) scanSourceFile(resolvedRoot, filename, violations);
  violations.sort((left, right) => (
    left.file.localeCompare(right.file)
    || left.line - right.line
    || left.column - right.column
    || left.rule.localeCompare(right.rule)
  ));
  return { scannedFiles: sourceFiles.length, violations };
}

function isMainModule() {
  return process.argv[1]
    && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
}

if (isMainModule()) {
  const report = scanToolInvocationBoundaries();
  if (report.violations.length === 0) {
    console.log(`Tool invocation boundary check passed (${report.scannedFiles} source files).`);
  } else {
    for (const violation of report.violations) {
      console.error(
        `${violation.file}:${violation.line}:${violation.column} `
        + `[${violation.rule}] ${violation.message}`,
      );
    }
    console.error(`Tool invocation boundary check failed (${report.violations.length} violations).`);
    process.exitCode = 1;
  }
}
