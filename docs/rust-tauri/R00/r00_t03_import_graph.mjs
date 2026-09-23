// R00-T03：只读扫描现役源码的导入边；不加载服务或用户数据。
// v2（R2 修复）：按 I1-I6 区分类型专用/混合导入、CJS require/createRequire/require.resolve、
// 动态 import 与顶层静态运行时求值；可达性分层为语法可达与静态运行时可达。
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import ts from 'typescript';

const root = path.resolve(import.meta.dirname, '../../..');
const output = path.join(import.meta.dirname, 'R00-T03_IMPORT_GRAPH.json');
const roots = [
  'server/main-full.ts', 'server/main-open.ts', 'server/index.ts',
  'server/bootstrap.ts', 'cli/entry.ts', 'desktop/main.cjs', 'desktop/bootstrap.cjs',
  'desktop/preload.cjs', 'desktop/src/main.tsx', 'desktop/src/mobile-main.tsx',
  'desktop/src/settings-main.tsx', 'desktop/src/quick-chat-main.tsx',
  'hub/index.ts', 'hub/channel-router.ts', 'hub/scheduler.ts',
  'core/bridge-session-manager.ts', 'lib/bridge/bridge-manager.ts',
];
const sourceDirs = ['core/', 'server/', 'lib/', 'hub/', 'cli/', 'desktop/', 'plugins/', 'scripts/', 'shared/'];
const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: root }).toString().split('\0').filter(Boolean);
const files = tracked.filter(file => sourceDirs.some(dir => file.startsWith(dir)))
  .filter(file => /\.(?:[cm]?js|tsx?)$/.test(file))
  .filter(file => !/(?:^|\/)(?:__tests__|dist|dist-renderer|dist-splash|dist-theme|node_modules)(?:\/|$)/.test(file))
  .filter(file => !/(?:\.test|\.spec)\.[cm]?[jt]sx?$/.test(file));
const fileSet = new Set(files);
const records = [];
const opaque = [];

function lineOf(source, node) { return source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1; }
function resolveLocal(from, spec) {
  if (!spec.startsWith('.')) return null;
  const base = path.posix.normalize(path.posix.join(path.posix.dirname(from), spec));
  for (const candidate of [base, `${base}.ts`, `${base}.tsx`, `${base}.js`, `${base}.cjs`, `${base}.mjs`, `${base}/index.ts`, `${base}/index.js`]) {
    if (fileSet.has(candidate)) return candidate;
  }
  return null;
}
function importedNames(clause) {
  if (!clause) return [];
  const result = [];
  if (clause.name) result.push({ imported: 'default', local: clause.name.text, type_only: !!clause.isTypeOnly });
  const bindings = clause.namedBindings;
  if (bindings && ts.isNamespaceImport(bindings)) result.push({ imported: '*', local: bindings.name.text, type_only: !!clause.isTypeOnly });
  if (bindings && ts.isNamedImports(bindings)) {
    for (const item of bindings.elements) result.push({ imported: item.propertyName?.text ?? item.name.text, local: item.name.text, type_only: !!(clause.isTypeOnly || item.isTypeOnly) });
  }
  return result;
}
// I2：export 侧必须合并父节点 isTypeOnly 与元素 isTypeOnly（R2 F01 的根因位）；
// `export * as N from`（NamespaceExport）与 `export * from`（星号再导出）分别保留本地名。
function exportedNames(node) {
  if (node.exportClause && ts.isNamedExports(node.exportClause)) {
    return node.exportClause.elements.map(item => ({
      imported: item.propertyName?.text ?? item.name.text, local: item.name.text,
      type_only: !!(node.isTypeOnly || item.isTypeOnly),
    }));
  }
  if (node.exportClause && ts.isNamespaceExport(node.exportClause)) {
    return [{ imported: '*', local: node.exportClause.name.text, type_only: !!node.isTypeOnly }];
  }
  return [{ imported: '*', local: '*', type_only: !!node.isTypeOnly }];
}
// 顶层判定：从节点向上到 SourceFile，途中出现函数体/块/控制流/类/条件表达式即视为
// 调用时执行；类成员初始化器按调用时处理（实例字段在构造时求值）。
const DEFERRING = new Set([
  ts.SyntaxKind.Block, ts.SyntaxKind.FunctionDeclaration, ts.SyntaxKind.FunctionExpression,
  ts.SyntaxKind.ArrowFunction, ts.SyntaxKind.MethodDeclaration, ts.SyntaxKind.Constructor,
  ts.SyntaxKind.GetAccessor, ts.SyntaxKind.SetAccessor, ts.SyntaxKind.IfStatement,
  ts.SyntaxKind.ForStatement, ts.SyntaxKind.ForOfStatement, ts.SyntaxKind.ForInStatement,
  ts.SyntaxKind.WhileStatement, ts.SyntaxKind.DoStatement, ts.SyntaxKind.SwitchStatement,
  ts.SyntaxKind.TryStatement, ts.SyntaxKind.CatchClause, ts.SyntaxKind.ClassDeclaration,
  ts.SyntaxKind.ClassExpression, ts.SyntaxKind.ConditionalExpression,
]);
function isModuleEvaluation(node, source) {
  let cur = node.parent;
  while (cur && cur !== source) {
    if (DEFERRING.has(cur.kind)) return false;
    cur = cur.parent;
  }
  return true;
}
function record(file, source, node, kind, spec, names = [], extra = {}) {
  const symbols = names;
  const typeOnly = symbols.length > 0 && symbols.every(s => s.type_only);
  let loadTiming;
  if (typeOnly) loadTiming = 'type_only';
  else if (kind === 'import' || kind === 'export_from') loadTiming = 'module_evaluation';
  else if (kind === 'require') loadTiming = isModuleEvaluation(node, source) ? 'module_evaluation' : 'call_time';
  else if (kind === 'dynamic_import') loadTiming = 'call_time';
  else loadTiming = 'resolve_only'; // require_resolve / import_meta_resolve 只解析路径，不执行模块
  records.push({ file, line: lineOf(source, node), kind, specifier: spec,
    resolved: resolveLocal(file, spec), symbols, type_only: typeOnly, load_timing: loadTiming, ...extra });
}
for (const file of files) {
  const content = fs.readFileSync(path.join(root, file), 'utf8');
  const source = ts.createSourceFile(file, content, ts.ScriptTarget.Latest, true,
    file.endsWith('.tsx') ? ts.ScriptKind.TSX : file.endsWith('.ts') ? ts.ScriptKind.TS : ts.ScriptKind.JS);
  // I3：登记 createRequire 绑定（含 `const require = createRequire(...)` 的影子 require）。
  // 全量遍历而非只看顶层语句：绑定可能位于函数体内（如 scripts/compute-cli-closure.mjs）。
  const createRequireBindings = new Set();
  function collectCreateRequireBindings(node) {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)
        && node.initializer && ts.isCallExpression(node.initializer)
        && ts.isIdentifier(node.initializer.expression)
        && node.initializer.expression.text === 'createRequire') {
      createRequireBindings.add(node.name.text);
    }
    ts.forEachChild(node, collectCreateRequireBindings);
  }
  collectCreateRequireBindings(source);
  function visit(node) {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      record(file, source, node, 'import', node.moduleSpecifier.text, importedNames(node.importClause));
    } else if (ts.isExportDeclaration(node) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) {
      record(file, source, node, 'export_from', node.moduleSpecifier.text, exportedNames(node));
    } else if (ts.isCallExpression(node)) {
      const arg = node.arguments[0];
      const isDynamic = node.expression.kind === ts.SyntaxKind.ImportKeyword;
      const isMetaResolve = ts.isPropertyAccessExpression(node.expression)
        && node.expression.expression.kind === ts.SyntaxKind.ImportKeyword
        && node.expression.name.text === 'resolve';
      // require.resolve / <createRequire 绑定>.resolve：只解析，不加载。
      const isBoundResolve = ts.isPropertyAccessExpression(node.expression)
        && ts.isIdentifier(node.expression.expression)
        && (node.expression.expression.text === 'require' || createRequireBindings.has(node.expression.expression.text))
        && node.expression.name.text === 'resolve';
      const isRequire = ts.isIdentifier(node.expression)
        && (node.expression.text === 'require' || createRequireBindings.has(node.expression.text));
      // createRequire(...)(...) 直接调用形态（如 shared/safe-fs.ts:37）。
      const isCreateRequireCallThrough = ts.isCallExpression(node.expression)
        && ts.isIdentifier(node.expression.expression) && node.expression.expression.text === 'createRequire';
      if (isDynamic || isRequire || isMetaResolve || isBoundResolve || isCreateRequireCallThrough) {
        const kind = isDynamic ? 'dynamic_import'
          : isMetaResolve ? 'import_meta_resolve'
          : isBoundResolve ? 'require_resolve'
          : 'require';
        const viaCreateRequire = isCreateRequireCallThrough
          || (isRequire && createRequireBindings.has(node.expression.text));
        if (arg && ts.isStringLiteralLike(arg)) {
          record(file, source, node, kind, arg.text, [], viaCreateRequire ? { via_create_require: true } : {});
        } else {
          opaque.push({ file, line: lineOf(source, node), kind, expression: arg?.getText(source).slice(0, 160) ?? null });
        }
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
}

// 语法可达（含类型专用、条件 require、动态 import 的解析边）与静态运行时可达
// （仅顶层静态 import/export_from 运行时边 + 顶层 require）分开计算。
const edges = new Map();
const staticRuntimeEdges = new Map();
for (const r of records) if (r.resolved) {
  if (!edges.has(r.file)) edges.set(r.file, new Set());
  edges.get(r.file).add(r.resolved);
  if (r.load_timing === 'module_evaluation'
      && (r.kind === 'import' || r.kind === 'export_from' || r.kind === 'require')) {
    if (!staticRuntimeEdges.has(r.file)) staticRuntimeEdges.set(r.file, new Set());
    staticRuntimeEdges.get(r.file).add(r.resolved);
  }
}
function reachability(edgeMap) {
  const paths = new Map();
  for (const entry of roots) {
    const queue = [[entry]];
    const seen = new Set();
    while (queue.length) {
      const chain = queue.shift();
      const at = chain.at(-1);
      if (seen.has(at)) continue;
      seen.add(at);
      if (!paths.has(at)) paths.set(at, {});
      paths.get(at)[entry] = chain;
      for (const next of edgeMap.get(at) ?? []) queue.push([...chain, next]);
    }
  }
  return paths;
}
const paths = reachability(edges);
const staticRuntimePaths = reachability(staticRuntimeEdges);
const isPi = spec => spec.startsWith('@earendil-works/pi-')
  || spec.startsWith('@mariozechner/pi-')
  || spec.includes('node_modules/@earendil-works/pi-')
  || spec.includes('node_modules/@mariozechner/pi-');
const isAdapter = r => r.resolved?.startsWith('lib/pi-sdk/') || r.specifier.includes('/pi-sdk/');
const sorted = a => a.sort((x, y) => x.file.localeCompare(y.file) || x.line - y.line || x.kind.localeCompare(y.kind));
const facadeFile = 'lib/pi-sdk/index.ts';
const facadeSource = ts.createSourceFile(facadeFile, fs.readFileSync(path.join(root, facadeFile), 'utf8'),
  ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const facadeExports = new Map();
for (const node of facadeSource.statements) {
  if (ts.isExportDeclaration(node) && node.exportClause && ts.isNamedExports(node.exportClause)) {
    for (const item of node.exportClause.elements) facadeExports.set(item.name.text,
      { symbol: item.name.text, line: lineOf(facadeSource, node), type_only: !!(node.isTypeOnly || item.isTypeOnly) });
  }
  if (!node.modifiers?.some(mod => mod.kind === ts.SyntaxKind.ExportKeyword)) continue;
  if (node.name && ts.isIdentifier(node.name)) facadeExports.set(node.name.text,
    { symbol: node.name.text, line: lineOf(facadeSource, node), type_only: ts.isInterfaceDeclaration(node) || ts.isTypeAliasDeclaration(node) });
  if (ts.isVariableStatement(node)) for (const decl of node.declarationList.declarations) {
    if (ts.isIdentifier(decl.name)) facadeExports.set(decl.name.text,
      { symbol: decl.name.text, line: lineOf(facadeSource, node), type_only: false });
  }
}
const facadeConsumers = new Map();
for (const r of records.filter(r => r.resolved === facadeFile && r.file !== facadeFile)) {
  for (const symbol of r.symbols) {
    const key = symbol.imported;
    if (!facadeConsumers.has(key)) facadeConsumers.set(key, []);
    facadeConsumers.get(key).push(`${r.file}:${r.line}`);
  }
}
const annotateReachability = r => ({
  ...r,
  reachable_from: Object.keys(paths.get(r.file) ?? {}),
  static_runtime_reachable_from: Object.keys(staticRuntimePaths.get(r.file) ?? {}),
});
const piConsumerFiles = [...paths].filter(([file]) => records.some(r => r.file === file && (isAdapter(r) || isPi(r.specifier)))).map(([file]) => file);
const staticPiConsumerFiles = [...staticRuntimePaths].filter(([file]) => records.some(r => r.file === file && (isAdapter(r) || isPi(r.specifier)))).map(([file]) => file);
const result = {
  schema: 'r00-t03-import-graph-v2',
  scope: 'git tracked application and scripts source under sourceDirs; excludes tests, generated bundles and node_modules; script-only edges are classified separately',
  semantics: {
    type_only: 'import/export 语句所有符号均为类型专用（import type、export type、元素级 type 前缀或 clause isTypeOnly）；类型边在 TS emit 后不产生运行时模块加载',
    load_timing: 'module_evaluation=顶层静态求值；call_time=函数体内/条件分支中的 require 或动态 import；type_only=类型专用；resolve_only=require.resolve/import.meta.resolve 仅解析路径不执行模块',
    reachable_from: '语法可达：沿全部可解析边（含类型专用、条件 require、动态 import）',
    static_runtime_reachable_from: '静态运行时可达：仅沿 load_timing=module_evaluation 的 import/export_from/require 边',
    via_create_require: '该 require 实为 createRequire 绑定或直接调用（ESM 文件内使用 CJS require）',
  },
  roots, scanned_files: files.length, parsed_import_edges: records.length,
  vendor_pi_imports: sorted(records.filter(r => isPi(r.specifier)).map(annotateReachability)),
  adapter_imports: sorted(records.filter(isAdapter).map(annotateReachability)),
  facade_public_exports: [...facadeExports.values()].sort((a, b) => a.symbol.localeCompare(b.symbol))
    .map(item => ({ ...item, static_consumers: facadeConsumers.get(item.symbol) ?? [],
      namespace_consumer_present: facadeConsumers.has('*') })),
  external_imports: sorted(records.filter(r => !r.specifier.startsWith('.') && !r.specifier.startsWith('node:'))
    .map(annotateReachability)),
  opaque_dynamic_imports: sorted(opaque),
  root_paths_to_pi_consumers: Object.fromEntries(
    [...paths].filter(([file]) => records.some(r => r.file === file && (isAdapter(r) || isPi(r.specifier)))).map(([file, origins]) => [file, origins])
  ),
  root_static_runtime_paths_to_pi_consumers: Object.fromEntries(
    [...staticRuntimePaths].filter(([file]) => records.some(r => r.file === file && (isAdapter(r) || isPi(r.specifier)))).map(([file, origins]) => [file, origins])
  ),
  reachability_summary: {
    pi_consumer_files_syntactic: piConsumerFiles.length,
    pi_consumer_files_static_runtime: staticPiConsumerFiles.length,
    files_reachable_from_main_full_syntactic: [...paths.values()].filter(origins => 'server/main-full.ts' in origins).length,
    files_reachable_from_main_full_static_runtime: [...staticRuntimePaths.values()].filter(origins => 'server/main-full.ts' in origins).length,
  },
};
fs.writeFileSync(output, `${JSON.stringify(result, null, 2)}\n`);
console.log(JSON.stringify({ output: path.relative(root, output), scanned_files: files.length,
  pi_vendor_imports: result.vendor_pi_imports.length, adapter_imports: result.adapter_imports.length,
  opaque_dynamic_imports: opaque.length, reachable_pi_consumers: Object.keys(result.root_paths_to_pi_consumers).length,
  static_runtime_pi_consumers: Object.keys(result.root_static_runtime_paths_to_pi_consumers).length }));
