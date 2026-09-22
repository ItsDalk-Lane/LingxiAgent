/**
 * check-core-contracts-strict.mjs — 本轮重构核心契约的 strict 类型门禁（P01-T04）。
 *
 * 与 `npm run typecheck`（root strict + node/test 宽松区）的关系：本检查显式
 * 列出核心契约文件，在 strict + noUncheckedIndexedAccess + exactOptionalPropertyTypes
 * 下编译（配置在 tsconfig.core-contracts.json）。include 之外的文件不会被假装
 * 覆盖；被 include 文件 import 进来的源码会随依赖图进入检查（TypeScript 的
 * 实际行为，exclude 不能假装隔离）。
 *
 * 用 TypeScript 编译器 API（不用子进程 tsc），便于测试注入额外文件（负例）。
 */
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TSCONTRACT_PATH = "tsconfig.core-contracts.json";

export function runCoreContractsTypeCheck({ extraFiles = [], configPath = path.join(rootDir, TSCONTRACT_PATH), sourceOverrides = {} } = {}) {
  const configFile = ts.readConfigFile(configPath, ts.sys.readFile);
  if (configFile.error) {
    throw new Error(`failed to read ${TSCONTRACT_PATH}: ${ts.flattenDiagnosticMessageText(configFile.error.messageText, " ")}`);
  }
  const parsed = ts.parseJsonConfigFileContent(configFile.config, ts.sys, path.dirname(configPath));
  if (parsed.errors.length > 0) {
    throw new Error(`failed to parse ${TSCONTRACT_PATH}: ${parsed.errors.map((e) => ts.flattenDiagnosticMessageText(e.messageText, " ")).join("; ")}`);
  }

  // 验收修复：显式注入的文件不存在时直接抛错，而不是让 createProgram 静默
  // 跳过（负例 fixture 被写错路径会让"注入负例必须失败"的测试假绿）。
  for (const extra of extraFiles) {
    const resolved = path.resolve(extra);
    if (!ts.sys.fileExists(resolved)) {
      throw new Error(`runCoreContractsTypeCheck: extra file does not exist: ${extra}`);
    }
  }

  // 精确入口被删除时 TypeScript 的 include 会静默漏掉它；本门禁必须失败。
  for (const entry of configFile.config.include ?? []) {
    if (typeof entry === "string" && !/[?*]/.test(entry)) {
      const resolved = path.resolve(path.dirname(configPath), entry);
      if (!ts.sys.fileExists(resolved)) throw new Error(`required core entry does not exist: ${entry}`);
    }
  }

  const rootNames = [...parsed.fileNames, ...extraFiles.map((f) => path.resolve(f))];
  // 仅供门禁自测在编译器内注入实际源码反例，不写磁盘，不暴露给产品入口。
  const host = ts.createCompilerHost(parsed.options);
  const originalReadFile = host.readFile.bind(host);
  const overrides = new Map(Object.entries(sourceOverrides).map(([file, text]) => [path.resolve(file), text]));
  for (const file of overrides.keys()) {
    if (!ts.sys.fileExists(file)) throw new Error(`source override file does not exist: ${file}`);
  }
  host.readFile = file => overrides.get(path.resolve(file)) ?? originalReadFile(file);
  const program = ts.createProgram(rootNames, parsed.options, host);
  const diagnostics = ts.getPreEmitDiagnostics(program);

  return {
    rootFiles: rootNames.map((f) => path.relative(rootDir, f)),
    // 报告真实编译闭包；第三方库单列，不能把入口数量当成覆盖数量。
    checkedFiles: program.getSourceFiles().filter(f => !program.isSourceFileDefaultLibrary(f) && !program.isSourceFileFromExternalLibrary(f)).map(f => path.relative(rootDir, f.fileName)).sort(),
    externalFiles: program.getSourceFiles().filter(f => program.isSourceFileFromExternalLibrary(f)).map(f => path.relative(rootDir, f.fileName)).sort(),
    diagnostics: diagnostics.map((diagnostic) => {
      const file = diagnostic.file;
      const position = file?.getLineAndCharacterOfPosition(diagnostic.start ?? 0);
      return {
        file: file ? path.relative(rootDir, file.fileName) : "<global>",
        line: position ? position.line + 1 : 0,
        character: position ? position.character + 1 : 0,
        code: `TS${diagnostic.code}`,
        message: ts.flattenDiagnosticMessageText(diagnostic.messageText, " "),
      };
    }).sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.character - b.character),
  };
}

function isMainModule() {
  return process.argv[1]
    && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
}

if (isMainModule()) {
  const report = runCoreContractsTypeCheck();
  if (report.diagnostics.length === 0) {
    console.log(`Core contracts strict check passed (${report.checkedFiles.length} files).`);
  } else {
    for (const d of report.diagnostics) {
      console.error(`${d.file}:${d.line}:${d.character} [${d.code}] ${d.message}`);
    }
    console.error(`Core contracts strict check failed (${report.diagnostics.length} diagnostics).`);
    process.exitCode = 1;
  }
}
