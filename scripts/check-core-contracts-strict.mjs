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

export function runCoreContractsTypeCheck({ extraFiles = [] } = {}) {
  const configPath = path.join(rootDir, TSCONTRACT_PATH);
  const configFile = ts.readConfigFile(configPath, ts.sys.readFile);
  if (configFile.error) {
    throw new Error(`failed to read ${TSCONTRACT_PATH}: ${ts.flattenDiagnosticMessageText(configFile.error.messageText, " ")}`);
  }
  const parsed = ts.parseJsonConfigFileContent(configFile.config, ts.sys, rootDir);
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

  const rootNames = [...parsed.fileNames, ...extraFiles.map((f) => path.resolve(f))];
  const program = ts.createProgram(rootNames, parsed.options);
  const diagnostics = ts
    .getPreEmitDiagnostics(program)
    .filter((diagnostic) => diagnostic.file !== undefined);

  return {
    checkedFiles: rootNames.map((f) => path.relative(rootDir, f)),
    diagnostics: diagnostics.map((diagnostic) => {
      const file = diagnostic.file;
      const position = file.getLineAndCharacterOfPosition(diagnostic.start ?? 0);
      return {
        file: path.relative(rootDir, file.fileName),
        line: position.line + 1,
        character: position.character + 1,
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
