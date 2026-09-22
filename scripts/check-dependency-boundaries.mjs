/**
 * check-dependency-boundaries.mjs — 模块依赖纪律静态门禁（P01-T06）。
 *
 * 与既有 scripts/check-tool-invocation-boundaries.mjs 的分工：那个管"工具执行
 * 旁路"（executeCanonical/callTool/executePluginTool 白名单），本检查管
 * "导入与依赖方向"：
 *
 *   sdk-direct-import      生产代码直接 import @earendil-works/*、@mariozechner/*
 *                          或裸 typebox（构造面）——必须收敛到 lib/pi-sdk 适配面。
 *   sdk-deep-path          生产代码 import node_modules/@earendil-works/** 深路径
 *                          （适配层内两处已登记例外；构建脚本不属生产扫描根）。
 *   host-into-core         core/lib/server/hub/shared/cli 导入 electron——桌面
 *                          宿主是唯一合法 Electron 层。
 *   adapter-reverse-dep    lib/pi-sdk 反向依赖 core/ 或 server/——适配层必须
 *                          保持向下（lib 层），不得读到编排层。
 *   sdk-dynamic-unproven   非 SDK 位置出现"计算式动态 import 且文件内出现 SDK
 *                          包名字符串"——静态无法证明安全（A12：动态未证安全
 *                          不得自动通过），需精确例外或改走适配面。
 *   host-dynamic-unproven  core-like 位置出现"计算式动态 import 且文件内出现
 *                          electron 字符串"——同 A12 保守裁决，用于 host 渗入。
 *
 * 规则基于 TypeScript AST 的真实导入语句（ImportDeclaration / ExportFrom /
 * CallExpression(import/require)），不是全文件字符串匹配——注释与普通字符串
 * 里的包名不会误报；但计算式动态 import 只能按上面的保守规则处理。
 *
 * 已知静态边界（验收加固后仍存在的）：字符串提及检测覆盖字面量与**常量拼接
 * 折叠**（二元 "+" 拼接与全字面量 substitution 的模板表达式 `` `a${'b'}c` ``
 * 均被求值），不覆盖运行时拼接（process.env、函数返回值、数组 join、含变量
 * substitution 的模板）——后者静态不可判定，登记为固有盲区；适配层内的动态
 * 加载属其职责，始终豁免。
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

export const DEPENDENCY_BOUNDARY_ALLOWLISTS = Object.freeze({
  // SDK 直导（含构造面 typebox 裸导入）唯一合法位置：适配层 + 已登记精确例外。
  sdkDirectImport: Object.freeze([
    "lib/pi-sdk/**",
    // P01-T02 登记：typebox 是根级直接依赖；网关参数 schema 校验基础设施消费
    // typebox/value 的运行时 Value。构造面 Type 仍必须经适配面。
    "lib/tools/invocation/schema-validator.ts",
  ]),
  // node_modules 深路径（SDK 未从包根导出的能力）在适配层内的已登记例外。
  sdkDeepPath: Object.freeze([
    "lib/pi-sdk/**",
  ]),
  // host-into-core 目前零例外；出现即违例。
  hostIntoCore: Object.freeze([]),
  // adapter-reverse-dep 零例外。
  adapterReverseDep: Object.freeze([]),
  // 计算式动态 import 提及 SDK 的：零例外（要加必须逐条给运行时证明）。
  sdkDynamicUnproven: Object.freeze([]),
  // 计算式动态 import 提及 electron 的（core-like 根）：零例外。
  hostDynamicUnproven: Object.freeze([]),
});

const SCAN_ROOTS = Object.freeze(["cli", "core", "desktop", "hub", "lib", "plugins", "server", "shared", "tools"]);
const CORE_LIKE_ROOTS = Object.freeze(new Set(["cli", "core", "hub", "lib", "server", "shared"]));
const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"]);

const SDK_PACKAGE_PATTERN = /^(?:@earendil-works|@mariozechner)\/(?:pi-ai|pi-coding-agent|pi-agent-core)(?:\/|$)/;
const SDK_DEEP_PATH_PATTERN = /node_modules\/@(?:earendil-works|mariozechner)\//;
// 提及检测的敏感片段：完整包名必然包含其 scope 片段，因此片段命中 ⊇ 完整名命中。
// 拼接构造（"@earendil-works" + "/pi-ai"）在常量折叠后同样落入这里。
const SDK_NAME_FRAGMENTS = ["@earendil-works", "@mariozechner", "pi-ai", "pi-coding-agent", "pi-agent-core"];
const HOST_NAME = "electron";

/** 常量字符串折叠：字面量、纯字面量 "+" 拼接与全字面量 substitution 的模板
 *  表达式可静态求值（`` `elect${''}ron` `` 与 "elect"+"ron" 静态等价，同受
 *  约束）；其余（变量/表达式 substitution、运行时拼接）返回 null。 */
function constantStringOf(node) {
  if (ts.isStringLiteralLike(node)) return node.text;
  if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.PlusToken) {
    const left = constantStringOf(node.left);
    const right = constantStringOf(node.right);
    return left !== null && right !== null ? left + right : null;
  }
  if (ts.isTemplateExpression(node)) {
    let folded = node.head.text;
    for (const span of node.templateSpans) {
      const literal = constantStringOf(span.expression);
      if (literal === null) return null;
      folded += literal + span.literal.text;
    }
    return folded;
  }
  return null;
}

function toPosixRelative(rootDir, filename) {
  return path.relative(rootDir, filename).split(path.sep).join("/");
}

function collectSourceFiles(rootDir) {
  const files = [];
  const visit = (directory) => {
    if (!fs.existsSync(directory)) return;
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      if (entry.name === "node_modules" || entry.name === "dist" || entry.name === "dist-renderer") continue;
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolute);
      } else if (entry.isFile() && SOURCE_EXTENSIONS.has(path.extname(entry.name))) {
        files.push(absolute);
      }
    }
  };
  for (const sourceRoot of SCAN_ROOTS) visit(path.join(rootDir, sourceRoot));
  return files.sort();
}

function allowlistMatches(allowlist, relativePath) {
  return allowlist.some((pattern) => (
    pattern.endsWith("/**")
      ? relativePath.startsWith(pattern.slice(0, -2))
      : relativePath === pattern
  ));
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
  const text = fs.readFileSync(filename, "utf8");
  const sourceFile = ts.createSourceFile(filename, text, ts.ScriptTarget.Latest, true, scriptKindFor(filename));

  const rootSegment = relativePath.split("/")[0] || "";
  const inAdapter = relativePath.startsWith("lib/pi-sdk/");
  const inCoreLike = CORE_LIKE_ROOTS.has(rootSegment);
  // 文件内出现过的敏感名字符串（字面量 + 常量拼接折叠），供计算式动态 import
  // 的保守判定（验收加固：拼接构造的包名不再自动放行）。
  let mentionsSensitiveSdkName = false;
  let mentionsElectron = false;
  const noteMention = (str) => {
    if (SDK_PACKAGE_PATTERN.test(str) || SDK_NAME_FRAGMENTS.some((f) => str.includes(f))) {
      mentionsSensitiveSdkName = true;
    }
    if (str.includes(HOST_NAME)) mentionsElectron = true;
  };

  const checkModuleSpecifier = (specifier, node, { dynamic = false } = {}) => {
    if (specifier === null) return; // 计算式，另行处理
    const isSdkPackage = SDK_PACKAGE_PATTERN.test(specifier);
    const isBareTypebox = specifier === "typebox";
    if ((isSdkPackage || isBareTypebox) && !allowlistMatches(DEPENDENCY_BOUNDARY_ALLOWLISTS.sdkDirectImport, relativePath)) {
      addViolation(
        violations, sourceFile, relativePath, node, "sdk-direct-import",
        `${dynamic ? "dynamic import" : "import"} of ${specifier} must stay inside lib/pi-sdk (registered exceptions apply).`,
      );
    }
    if (SDK_DEEP_PATH_PATTERN.test(specifier) && !allowlistMatches(DEPENDENCY_BOUNDARY_ALLOWLISTS.sdkDeepPath, relativePath)) {
      addViolation(
        violations, sourceFile, relativePath, node, "sdk-deep-path",
        `deep node_modules path import (${specifier.slice(0, 80)}) is only allowed inside the SDK adapter layer.`,
      );
    }
    const isElectron = specifier === "electron" || specifier.startsWith("electron/");
    if (isElectron && CORE_LIKE_ROOTS.has(rootSegment) && !allowlistMatches(DEPENDENCY_BOUNDARY_ALLOWLISTS.hostIntoCore, relativePath)) {
      addViolation(
        violations, sourceFile, relativePath, node, "host-into-core",
        "core/lib/server/hub/shared/cli must not import electron; the desktop host is the only Electron layer.",
      );
    }
    if (inAdapter && (specifier.startsWith("../core/") || specifier.startsWith("../../core/")
      || specifier.startsWith("../server/") || specifier.startsWith("../../server/"))) {
      addViolation(
        violations, sourceFile, relativePath, node, "adapter-reverse-dep",
        "lib/pi-sdk must not import from core/ or server/ (adapter stays below the orchestration layer).",
      );
    }
  };

  const visit = (node) => {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      checkModuleSpecifier(node.moduleSpecifier.text, node);
    } else if (ts.isExportDeclaration(node) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) {
      checkModuleSpecifier(node.moduleSpecifier.text, node);
    } else if (ts.isCallExpression(node)) {
      const expression = node.expression.getText(sourceFile);
      if (expression === "require" && node.arguments.length === 1) {
        visitDynamicArgument(node.arguments[0], node, "require");
      } else if (expression === "import" || ts.isImportKeyword?.(node.expression)) {
        if (node.arguments.length === 1) {
          visitDynamicArgument(node.arguments[0], node, "import()");
        }
      }
    } else if (ts.isStringLiteral(node)) {
      noteMention(node.text);
    } else if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.PlusToken) {
      const folded = constantStringOf(node);
      if (folded !== null) noteMention(folded);
    } else if (ts.isTemplateExpression(node)) {
      const folded = constantStringOf(node);
      if (folded !== null) noteMention(folded);
    }
    ts.forEachChild(node, visit);
  };

  // import()/require 参数：字面量或常量折叠成功 → 按具名 specifier 检查；
  // 折叠不出（真计算式）→ 记为候选，收尾按文件级敏感提及保守裁决。
  const visitDynamicArgument = (arg, node, kind) => {
    const folded = constantStringOf(arg);
    if (folded !== null) {
      noteMention(folded);
      checkModuleSpecifier(folded, node, { dynamic: true });
    } else {
      visitComputedDynamic(node, kind);
    }
  };

  // 计算式动态 import/require：静态无法证明目标。若文件内（任何位置）出现
  // SDK 名或（core-like 根内）electron 字符串，保守判违例——要合法必须给
  // 精确例外+运行时证明。
  const visitComputedDynamic = (node, kind) => {
    if (inAdapter) return; // 适配层内部资源加载属其职责
    computedDynamicCandidates.push({ node, kind });
  };
  const computedDynamicCandidates = [];
  const settleComputedDynamic = () => {
    for (const { node, kind } of computedDynamicCandidates) {
      if (mentionsSensitiveSdkName) {
        addViolation(
          violations, sourceFile, relativePath, node, "sdk-dynamic-unproven",
          `computed ${kind} in a file that also mentions an SDK package name cannot be statically proven safe; route it through lib/pi-sdk or register a precise exception with runtime proof.`,
        );
      }
      if (inCoreLike && mentionsElectron) {
        addViolation(
          violations, sourceFile, relativePath, node, "host-dynamic-unproven",
          `computed ${kind} in a core-like file that also mentions "${HOST_NAME}" cannot be statically proven safe; the desktop host is the only Electron layer.`,
        );
      }
    }
  };

  visit(sourceFile);
  settleComputedDynamic();
}

function scriptKindFor(filename) {
  if (filename.endsWith(".tsx")) return ts.ScriptKind.TSX;
  if (filename.endsWith(".jsx")) return ts.ScriptKind.JSX;
  if (filename.endsWith(".js") || filename.endsWith(".mjs") || filename.endsWith(".cjs")) {
    return ts.ScriptKind.JS;
  }
  return ts.ScriptKind.TS;
}

export function scanDependencyBoundaries({ rootDir = process.cwd() } = {}) {
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
  const report = scanDependencyBoundaries();
  if (report.violations.length === 0) {
    console.log(`Dependency boundary check passed (${report.scannedFiles} source files).`);
  } else {
    for (const violation of report.violations) {
      console.error(
        `${violation.file}:${violation.line}:${violation.column} `
        + `[${violation.rule}] ${violation.message}`,
      );
    }
    console.error(`Dependency boundary check failed (${report.violations.length} violations).`);
    process.exitCode = 1;
  }
}
