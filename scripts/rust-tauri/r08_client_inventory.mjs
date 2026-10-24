#!/usr/bin/env node
// 静态盘点现役页面的宿主直连与适配入口；不执行界面，不把盘点当验收。
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
const require = createRequire(import.meta.url);
const ts = require('typescript');
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const output = path.join(root, 'docs/rust-tauri/R08/CLIENT_BOUNDARY_PREPARATION.json');
const relative = file => path.relative(root, file).replaceAll(path.sep, '/');
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const ignored = file => /(?:^|\/)(?:__tests__|tests|__fixtures__|node_modules|dist[^/]*)\//.test(file) || /\.(?:test|spec|stories)\.[cm]?[jt]sx?$/.test(file);
const script = file => /\.[cm]?[jt]sx?$/.test(file) && !/\.d\.[cm]?ts$/.test(file);
const walk = folder => fs.readdirSync(folder, { withFileTypes: true }).flatMap(row => {
  const file = path.join(folder, row.name);
  return ignored(relative(file) + (row.isDirectory() ? '/' : '')) ? [] : row.isDirectory() ? walk(file) : script(file) ? [file] : [];
});
const codeFiles = walk(path.join(root, 'desktop/src'));
const preload = path.join(root, 'desktop/preload.cjs');
const types = path.join(root, 'desktop/src/react/types.ts');
const options = { allowJs: true, checkJs: false, noEmit: true, skipLibCheck: true, jsx: ts.JsxEmit.Preserve, target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler, types: [] };
const program = ts.createProgram([...codeFiles, preload], options);
const checker = program.getTypeChecker();
const cache = new Map();
const source = file => {
  if (!cache.has(file)) cache.set(file, program.getSourceFile(file) || ts.createSourceFile(file, fs.readFileSync(file, 'utf8'), options.target, true));
  return cache.get(file);
};
const visit = (node, fn) => { fn(node); ts.forEachChild(node, child => visit(child, fn)); };
const unwrap = node => {
  while (node && (ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isNonNullExpression(node) || ts.isTypeAssertionExpression(node) || ts.isSatisfiesExpression(node))) node = node.expression;
  return node;
};
const property = node => ts.isPropertyAccessExpression(node) ? node.name.text : ts.isElementAccessExpression(node) && node.argumentExpression && ts.isStringLiteralLike(node.argumentExpression) ? node.argumentExpression.text : null;
const location = node => {
  const sf = node.getSourceFile(), p = sf.getLineAndCharacterOfPosition(node.getStart(sf));
  return { file: relative(sf.fileName), line: p.line + 1, column: p.character + 1 };
};
const symbol = node => {
  const result = node && checker.getSymbolAtLocation(node);
  return result && result.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(result) : result;
};

// HTML 仅用于识别页面入口；代码引用、调用和别名均以 TypeScript AST 判断。
const pages = fs.readdirSync(path.join(root, 'desktop/src')).filter(name => name.endsWith('.html')).sort().map(name => {
  const file = path.join(root, 'desktop/src', name), content = fs.readFileSync(file, 'utf8');
  return { file: relative(file), sha256: sha(file), scriptEntrypoints: [...content.matchAll(/<script\b[^>]*\bsrc=["']([^"']+)["'][^>]*>/g)].map(match => path.resolve(path.dirname(file), match[1])) };
});
// HTML中的theme.js由独立构建生成；开发服务器也把同一标签改到这份源码。
const bundleEntrypointMappings = [{
  requestedScript: 'desktop/src/lib/theme.js', source: 'desktop/src/shared/theme.ts',
  buildConfig: 'vite.config.theme.js', devRewriteConfig: 'vite.config.ts',
  basis: 'build.lib.entry + fileName theme.js；useSourceThemeInDev使用同一源码',
}];
const reachable = new Set(), imports = [], unresolvedDynamicImports = [];
const queue = pages.flatMap(page => page.scriptEntrypoints).filter(file => fs.existsSync(file) && script(file));
for (const mapping of bundleEntrypointMappings) if (pages.some(page => page.scriptEntrypoints.includes(path.join(root, mapping.requestedScript)))) queue.push(path.join(root, mapping.source));
while (queue.length) {
  const file = queue.shift();
  if (reachable.has(file) || ignored(relative(file))) continue;
  reachable.add(file);
  visit(source(file), node => {
    let specifier;
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) specifier = node.moduleSpecifier;
    else if (ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword || ts.isIdentifier(node.expression) && node.expression.text === 'require')) {
      specifier = node.arguments[0];
      if (!specifier || !ts.isStringLiteralLike(specifier)) unresolvedDynamicImports.push({ ...location(node), expression: node.getText(source(file)) });
    }
    if (!specifier || !ts.isStringLiteralLike(specifier) || !specifier.text.startsWith('.')) return;
    const resolved = ts.resolveModuleName(specifier.text, file, options, ts.sys).resolvedModule?.resolvedFileName;
    if (resolved && resolved.startsWith(root + path.sep) && !resolved.includes('/node_modules/') && script(resolved)) {
      imports.push({ from: relative(file), to: relative(resolved), specifier: specifier.text });
      queue.push(resolved);
    }
  });
}
const renderer = [...reachable].filter(file => file.startsWith(path.join(root, 'desktop/src/') )).sort();
const aliases = new Map(), functionResults = new Map();
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
function infer(expression) {
  const node = unwrap(expression);
  if (!node) return null;
  if (ts.isIdentifier(node)) return aliases.get(symbol(node)) || null;
  if (ts.isPropertyAccessExpression(node) || ts.isElementAccessExpression(node)) {
    const base = unwrap(node.expression), name = property(node);
    if (ts.isIdentifier(base) && ['window', 'globalThis', 'self'].includes(base.text) && ['hana', 'platform'].includes(name)) return { root: name };
    const parent = infer(base);
    return parent && !parent.member ? { root: parent.root, member: name ?? '<dynamic>' } : null;
  }
  if (ts.isCallExpression(node)) return functionResults.get(symbol(unwrap(node.expression))) || null;
  if (ts.isConditionalExpression(node)) {
    const branches = [infer(node.whenTrue), infer(node.whenFalse)].filter(Boolean);
    return branches.length && branches.every(value => same(value, branches[0])) ? branches[0] : null;
  }
  if (ts.isBinaryExpression(node) && [ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.AmpersandAmpersandToken].includes(node.operatorToken.kind)) {
    const candidates = [infer(node.left), infer(node.right)].filter(Boolean);
    return candidates.length && candidates.every(value => same(value, candidates[0])) ? candidates[0] : null;
  }
  return null;
}
let changed = true, rounds = 0;
while (changed && rounds++ < 30) {
  changed = false;
  const set = (map, key, value) => { if (key && value && !same(map.get(key), value)) { map.set(key, value); changed = true; } };
  for (const file of codeFiles) visit(source(file), node => {
    if (ts.isVariableDeclaration(node) && node.initializer) {
      const value = infer(node.initializer);
      if (ts.isIdentifier(node.name)) set(aliases, symbol(node.name), value);
      if (ts.isObjectBindingPattern(node.name) && value && !value.member) for (const item of node.name.elements) if (ts.isIdentifier(item.name)) set(aliases, symbol(item.name), { root: value.root, member: item.propertyName?.getText(source(file)) || item.name.text });
    }
    if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken && ts.isIdentifier(node.left)) set(aliases, symbol(node.left), infer(node.right));
    if ((ts.isFunctionDeclaration(node) || ts.isArrowFunction(node) || ts.isFunctionExpression(node)) && node.body) {
      const returns = [];
      if (!ts.isBlock(node.body)) returns.push(infer(node.body));
      else {
        const examine = child => {
          if (ts.isFunctionLike(child)) return;
          if (ts.isReturnStatement(child) && child.expression) returns.push(infer(child.expression));
          ts.forEachChild(child, examine);
        };
        ts.forEachChild(node.body, examine);
      }
      const values = returns.filter(Boolean);
      const name = node.name || (ts.isVariableDeclaration(node.parent) ? node.parent.name : null);
      if (values.length && values.every(value => same(value, values[0]))) set(functionResults, symbol(name), values[0]);
    }
  });
}
if (changed) throw Error('别名传播未收敛，不得输出貌似完整的清单');

const bootstrapMembers = new Set(['getServerPort', 'getServerToken', 'getServerConnectionInfo', 'onServerRestarted']);
const resourceMembers = new Set(['readFile', 'writeFile', 'readFileBase64', 'readFileSnapshot', 'writeFileIfUnchanged', 'writeFileBinary', 'copyFile', 'watchFile', 'unwatchFile', 'onFileChanged', 'watchWorkspace', 'unwatchWorkspace', 'onWorkspaceChanged', 'readDocxHtml', 'readXlsxHtml', 'listSkillFiles', 'readSkillFile']);
const browserMembers = new Set(['rustBrowserRequest', 'setBrowserConnection', 'getBrowserViewerContext', 'browserGoBack', 'browserGoForward', 'browserReload', 'browserNewTab', 'browserSwitchTab', 'browserCloseTab', 'browserEmergencyStop', 'onBrowserUpdate', 'closeBrowser']);
const responsibility = member => bootstrapMembers.has(member) ? 'CONNECTION_BOOTSTRAP_TO_SERVICE_CLIENT' : resourceMembers.has(member) ? 'MIXED_RESOURCE_IO_REQUIRES_EXECUTION_NODE_SPLIT' : browserMembers.has(member) ? 'MIXED_BROWSER_HOST_RELAY_RUST_REMAINS_AUTHORITY' : 'DESKTOP_HOST_CAPABILITY';
const roots = [], accesses = [], calls = [], electronImports = [], aliasBindings = [];
for (const file of codeFiles) visit(source(file), node => {
  if ((ts.isPropertyAccessExpression(node) || ts.isElementAccessExpression(node))) {
    const value = infer(node);
    if (value && !value.member) roots.push({ ...location(node), root: value.root, expression: node.getText(source(file)), staticRendererReachable: reachable.has(file) });
    if (value?.member) {
      const row = { ...location(node), ...value, expression: node.getText(source(file)), staticRendererReachable: reachable.has(file), directRoot: !ts.isIdentifier(unwrap(node.expression)), responsibility: responsibility(value.member) };
      let outer = node;
      while (outer.parent && unwrap(outer.parent) === node) outer = outer.parent;
      row.invoked = ts.isCallExpression(outer.parent) && unwrap(outer.parent.expression) === node;
      row.optional = !!node.questionDotToken || !!(ts.isCallExpression(outer.parent) && outer.parent.questionDotToken);
      accesses.push(row);
      if (row.invoked) calls.push(row);
    }
  }
  if (ts.isCallExpression(node) && ts.isIdentifier(unwrap(node.expression))) {
    const value = infer(node.expression);
    if (value?.member) calls.push({ ...location(node), ...value, expression: node.getText(source(file)), staticRendererReachable: reachable.has(file), directRoot: false, invoked: true, optional: !!node.questionDotToken, responsibility: responsibility(value.member), form: 'destructured_or_callable_alias' });
  }
  if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && aliases.has(symbol(node.name))) aliasBindings.push({ ...location(node), identifier: node.name.text, ...aliases.get(symbol(node.name)), initializer: node.initializer?.getText(source(file)) || null });
  const module = ts.isImportDeclaration(node) ? node.moduleSpecifier : ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'require' ? node.arguments[0] : null;
  if (module && ts.isStringLiteralLike(module) && module.text === 'electron') electronImports.push({ ...location(node), expression: node.getText(source(file)), staticRendererReachable: reachable.has(file) });
});

function constantResult(fn) {
  if (!fn || !(ts.isArrowFunction(fn) || ts.isFunctionExpression(fn) || ts.isMethodDeclaration(fn))) return null;
  let body = unwrap(fn.body);
  if (ts.isBlock(body)) {
    if (!body.statements.length) return 'empty_body';
    if (body.statements.length === 1 && ts.isReturnStatement(body.statements[0])) body = unwrap(body.statements[0].expression);
    else return null;
  }
  if (!body) return 'undefined';
  if ([ts.SyntaxKind.FalseKeyword, ts.SyntaxKind.TrueKeyword, ts.SyntaxKind.NullKeyword].includes(body.kind)) return body.getText();
  if (ts.isIdentifier(body) && ['undefined', 'noopUnsubscribe'].includes(body.text)) return body.text;
  if (ts.isArrayLiteralExpression(body) && !body.elements.length) return '[]';
  if (ts.isObjectLiteralExpression(body)) return body.getText();
  if (ts.isStringLiteralLike(body)) return JSON.stringify(body.text);
  return null;
}
function objectMembers(object) {
  return object.properties.map(item => {
    const name = item.name?.getText().replace(/^['"]|['"]$/g, '') || '<spread-or-dynamic>';
    const fn = ts.isPropertyAssignment(item) ? item.initializer : item;
    const ipc = [], api = [], httpNonOkFallbacks = [], emptyCatchBlocks = [];
    visit(item, node => {
      if (ts.isCatchClause(node) && node.block.statements.length === 0) emptyCatchBlocks.push(location(node));
      if (ts.isConditionalExpression(node) && ts.isPropertyAccessExpression(node.condition) && node.condition.name.text === 'ok') httpNonOkFallbacks.push({ ...location(node), expression: node.getText() });
      if (!ts.isCallExpression(node)) return;
      if (ts.isPropertyAccessExpression(node.expression) && ts.isIdentifier(node.expression.expression) && node.expression.expression.text === 'ipcRenderer') ipc.push({ method: node.expression.name.text, channel: node.arguments[0] && ts.isStringLiteralLike(node.arguments[0]) ? node.arguments[0].text : '<dynamic>' });
      if (ts.isIdentifier(node.expression) && node.expression.text === 'apiFetch') api.push(node.arguments[0]?.getText() || '<missing>');
    });
    return { ...location(item), name, optional: !!item.questionToken, responsibility: responsibility(name), implementation: fn.getText(), constantResult: constantResult(fn), ipc, httpRoutes: api, httpNonOkFallbacks, emptyCatchBlocks };
  });
}
let bridge = [], web = [], mobile = [], apiType = [];
visit(source(preload), node => {
  if (ts.isCallExpression(node) && node.arguments[0] && ts.isStringLiteralLike(node.arguments[0]) && node.arguments[0].text === 'hana' && node.arguments[1] && ts.isObjectLiteralExpression(node.arguments[1])) bridge = objectMembers(node.arguments[1]);
});
const webHostFile = path.join(root, 'desktop/src/react/host/web-desktop-host.ts');
// R08-T02：Web fallback 成员对象已迁入宿主层 web-desktop-host.ts（createWebDesktopHost 内的 api 字面量）；
// modules/platform.js 只剩先于 React 的可视引导与 Electron 别名安装，不再含成员对象字面量。
visit(source(webHostFile), node => {
  if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text === 'api' && node.initializer) {
    const literal = unwrap(node.initializer);
    if (ts.isObjectLiteralExpression(literal)) web = objectMembers(literal);
  }
});
visit(source(path.join(root, 'desktop/src/react/mobile/mobile-platform.ts')), node => {
  if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text === 'api' && node.initializer && ts.isObjectLiteralExpression(node.initializer)) mobile = objectMembers(node.initializer);
});
visit(source(types), node => {
  if (ts.isInterfaceDeclaration(node) && node.name.text === 'PlatformApi') apiType = node.members.map(member => ({ ...location(member), name: member.name?.getText() || '<index-signature>', optional: !!member.questionToken, signature: member.getText(), responsibility: responsibility(member.name?.getText() || '') }));
});
if (!bridge.length || !web.length || !mobile.length || !apiType.length) throw Error('必须找全四份现有适配/类型对象，不能接受空扫描');
const names = rows => new Set(rows.map(row => row.name));
const bridgeNames = names(bridge), webNames = names(web), typeNames = names(apiType.filter(row => row.name !== '<index-signature>'));
for (const row of [...web, ...mobile]) {
  row.consumerInvocations = calls.filter(call => call.member === row.name && call.staticRendererReachable).map(({ file, line, root, optional }) => ({ file, line, root, optional }));
  row.visibleOutcomeStatus = 'STATIC_CONSUMERS_LISTED_NOT_EXECUTED';
}
const rustAdapters = renderer.filter(file => /\/react\/services\/rust-[^/]+\.ts$/.test(file)).map(file => {
  const exports = [];
  visit(source(file), node => {
    if (ts.isFunctionDeclaration(node) && node.name && node.modifiers?.some(mod => mod.kind === ts.SyntaxKind.ExportKeyword)) exports.push({ name: node.name.text, ...location(node) });
  });
  return { file: relative(file), sha256: sha(file), exports, importedBy: imports.filter(row => row.to === relative(file)).map(row => row.from), status: 'STATIC_REUSE_CANDIDATE_NOT_ACCEPTED_RUNTIME' };
});
const countBy = (rows, key) => Object.fromEntries([...new Set(rows.map(row => row[key]))].sort().map(value => [value, rows.filter(row => row[key] === value).length]));

// ---- R08-T02 批次 E：边界断言 ----
// 白名单：宿主适配层与既有引导文件允许直接读 window.hana / window.platform；
// 其余任何非测试源码的直接根引用/成员访问/调用都是违例。
const boundaryWhitelistPrefixes = [
  'desktop/src/react/host/',
  'desktop/src/modules/platform.js',
  'desktop/src/shared/theme.ts',
  'desktop/src/react/mobile/mobile-platform.ts',
];
// Lead T01 范围文件（含既有动态 import 结构）：不计入违例，但仍列清单。
// 另有两份 main 进程专用 .cjs（由 desktop/main.cjs / bootstrap.cjs require，从不进渲染入口构建）：
// 其 require('electron') 是主进程 API 合法使用，非渲染直连。
const boundaryWhitelistFiles = [
  'desktop/src/react/settings/tabs/observability/trace-detail/TraceDetailOverlay.tsx',
  'desktop/src/office-pdf-helper.cjs',
  'desktop/src/shared/splash-avatar-path.cjs',
];
const boundaryWhitelistFileReasons = {
  'desktop/src/react/settings/tabs/observability/trace-detail/TraceDetailOverlay.tsx': 'Lead T01 范围；既有 services/rust-chat-history 动态 import 结构',
  'desktop/src/office-pdf-helper.cjs': 'main 进程 PDF 助手子进程入口（desktop/bootstrap.cjs require），非渲染代码',
  'desktop/src/shared/splash-avatar-path.cjs': 'main 进程 IPC 注册助手（desktop/main.cjs require），非渲染代码',
};
const whitelisted = rel => boundaryWhitelistPrefixes.some(prefix => rel.startsWith(prefix)) || boundaryWhitelistFiles.includes(rel);
const productionViolations = [
  ...roots.filter(row => !whitelisted(row.file)).map(row => ({ ...row, rule: 'DIRECT_WINDOW_BRIDGE_ROOT_OUTSIDE_WHITELIST' })),
  ...accesses.filter(row => !whitelisted(row.file)).map(row => ({ ...row, rule: 'DIRECT_WINDOW_BRIDGE_ACCESS_OUTSIDE_WHITELIST' })),
  ...calls.filter(row => !whitelisted(row.file)).map(row => ({ ...row, rule: 'DIRECT_WINDOW_BRIDGE_CALL_OUTSIDE_WHITELIST' })),
  ...electronImports.filter(row => !whitelisted(row.file)).map(row => ({ ...row, rule: 'RENDERER_DIRECT_ELECTRON_IMPORT' })),
];
// 测试文件语法级直连审计（生产别名分析不覆盖测试）：赋值（桩安装）收编为记录；
// 读取（成员访问/调用/解构/传递）单列清单。测试读取暂不计入阻断违例：
// composer-send.transaction.test.ts / R02RustWebSocket.test.ts 属 Lead T01 在飞范围不可触碰，
// 其余测试断言读取迁移列入后续批次（本文件 PASS/FAIL 只断言生产边界）。
const isTestPath = rel => /(?:^|\/)(?:__tests__|tests)\//.test(rel) || /\.(?:test|spec)\.[cm]?[jt]sx?$/.test(rel);
const walkTests = folder => fs.readdirSync(folder, { withFileTypes: true }).flatMap(row => {
  const file = path.join(folder, row.name);
  const rel = relative(file) + (row.isDirectory() ? '/' : '');
  if (/(?:^|\/)(?:node_modules|dist[^/]*|\.[^/]*)\//.test(rel)) return [];
  return row.isDirectory() ? walkTests(file) : script(file) && isTestPath(rel) ? [file] : [];
});
const scriptKind = file => /\.tsx$/.test(file) ? ts.ScriptKind.TSX : /\.ts$/.test(file) ? ts.ScriptKind.TS : /\.jsx$/.test(file) ? ts.ScriptKind.JSX : ts.ScriptKind.JS;
const testStubAssignments = [], testDirectReads = [];
for (const file of walkTests(path.join(root, 'desktop/src'))) {
  const sf = ts.createSourceFile(file, fs.readFileSync(file, 'utf8'), options.target, true, scriptKind(file));
  visit(sf, node => {
    if (!(ts.isPropertyAccessExpression(node) || ts.isElementAccessExpression(node))) return;
    const base = unwrap(node.expression), name = property(node);
    if (!(base && ts.isIdentifier(base) && ['window', 'globalThis', 'self'].includes(base.text) && ['hana', 'platform'].includes(name))) return;
    // 只记录链根一次：window.platform.foo.bar 的违例上下文取整条链的最外层。
    let top = node;
    while (top.parent && (ts.isPropertyAccessExpression(top.parent) || ts.isElementAccessExpression(top.parent)) && unwrap(top.parent.expression) === top) top = top.parent;
    const assignment = !!(top.parent && ts.isBinaryExpression(top.parent) && top.parent.operatorToken.kind === ts.SyntaxKind.EqualsToken && unwrap(top.parent.left) === top);
    const row = { ...location(node), root: name, expression: top.getText(sf).slice(0, 160), context: assignment ? 'assignment_stub' : 'read' };
    (assignment ? testStubAssignments : testDirectReads).push(row);
  });
}
const boundary = {
  rules: [
    { id: 'PRODUCTION_NO_DIRECT_WINDOW_BRIDGE_OUTSIDE_WHITELIST', blocking: true, scope: 'desktop/src 非测试源码', detail: 'window.hana / window.platform 的根引用、成员访问、调用只允许出现在白名单宿主适配文件' },
    { id: 'RENDERER_NO_DIRECT_ELECTRON_IMPORT', blocking: true, scope: 'desktop/src 非测试源码', detail: "非测试渲染源码不得 import/require 'electron'" },
    { id: 'TEST_ASSIGNMENT_STUBS_COLLECTED_NOT_VIOLATIONS', blocking: false, scope: 'desktop/src 测试源码', detail: 'window.hana / window.platform 的赋值视为宿主桩安装，收编记录；读取单列 inventory，待 T01 落地后迁移' },
  ],
  whitelistPrefixes: boundaryWhitelistPrefixes,
  whitelistFiles: boundaryWhitelistFiles,
  whitelistFileReasons: boundaryWhitelistFileReasons,
  productionViolations,
  testStubAssignments,
  testDirectReads,
  testDirectReadFiles: [...new Set(testDirectReads.map(row => row.file))].sort(),
  leadT01ScopeTestFiles: ['desktop/src/react/__tests__/components/composer-send.transaction.test.ts', 'desktop/src/react/__tests__/components/R02RustWebSocket.test.ts'],
  result: productionViolations.length ? 'FAIL' : 'PASS',
};
const inputFiles = [...new Set([...codeFiles, preload, types, webHostFile, path.join(root, 'vite.config.ts'), path.join(root, 'vite.config.splash.ts'), path.join(root, 'vite.config.theme.js'), fileURLToPath(import.meta.url)])].sort();
const reachableHanaCalls = calls.filter(row => row.root === 'hana' && row.staticRendererReachable);
const result = {
  schema: 'lingxi.r08.client-boundary-preparation.v1', stage: 'R08', status: 'PREPARATION_ONLY_NOT_A03_OR_A04_PASS',
  sourceSha: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
  method: { parser: `TypeScript ${ts.version} AST`, entrypoints: 'desktop/src 顶层八份现有HTML脚本入口，含独立构建splash及theme生成源码映射；递归静态相对import/export/require与字面量dynamic import', aliasAnalysis: '按TypeScript词法符号迭代传播变量/解构/赋值/直接返回包装；剥离括号、类型断言、非空断言；条件表达式保留同源分支', excluded: 'tests/__tests__/__fixtures__/node_modules/dist*及.test/.spec/.stories；不把注释或普通字符串算访问/调用', boundary: '静态可达不代表运行分支可达；不证明完整跨函数参数/对象字段/反射数据流；不以接口字符串判断语义通过', aliasRounds: rounds },
  counts: { scannedRendererFiles: renderer.length, originalNonTestDesktopSourceCandidates: codeFiles.length, rootReferences: roots.filter(row => row.staticRendererReachable).length, hanaRootReferences: roots.filter(row => row.root === 'hana' && row.staticRendererReachable).length, hanaMemberAccesses: accesses.filter(row => row.root === 'hana' && row.staticRendererReachable).length, hanaInvocations: reachableHanaCalls.length, hanaInvocationFiles: new Set(reachableHanaCalls.map(row => row.file)).size, allNonTestHanaInvocations: calls.filter(row => row.root === 'hana').length, notStaticallyReachedHanaInvocations: calls.filter(row => row.root === 'hana' && !row.staticRendererReachable).length, platformInvocations: calls.filter(row => row.root === 'platform' && row.staticRendererReachable).length, rendererElectronImports: electronImports.filter(row => row.staticRendererReachable).length, preloadMembers: bridge.length, platformTypeNamedMembers: typeNames.size, webMembers: web.length, webConstantOrEmptyMembers: web.filter(row => row.constantResult !== null).length, mobileMembers: mobile.length, rustServiceAdapters: rustAdapters.length },
  pages: pages.map(page => ({ ...page, scriptEntrypoints: page.scriptEntrypoints.map(relative) })),
  bundleEntrypointMappings,
  sourceBindings: Object.fromEntries(inputFiles.map(file => [relative(file), sha(file)])),
  rootReferences: roots, memberAccesses: accesses, invocations: calls, aliases: aliasBindings,
  hanaInvocationsByMember: countBy(reachableHanaCalls, 'member'),
  hanaInvocationsByFile: countBy(reachableHanaCalls, 'file'),
  notStaticallyReachedHanaInvocations: calls.filter(row => row.root === 'hana' && !row.staticRendererReachable),
  rendererElectronImports: electronImports, unresolvedDynamicImports,
  preload: { file: relative(preload), actualBundle: 'desktop/preload.bundle.cjs（由vite.config.preload.js生成；当前仅读源码）', members: bridge },
  platformApi: { file: relative(types), members: apiType, indexSignatureAllowsUnlistedMembers: apiType.some(row => row.name === '<index-signature>'), preloadMembersMissingExplicitType: [...bridgeNames].filter(name => !typeNames.has(name)), typedMembersMissingPreload: [...typeNames].filter(name => !bridgeNames.has(name)) },
  webFallback: { file: 'desktop/src/react/host/web-desktop-host.ts', bootstrapFile: 'desktop/src/modules/platform.js（仅可视引导 + Electron 下 window.platform=window.hana 抢先别名；宿主安装与收编在 react/host/）', electronBranch: 'preload 桥本体经 DesktopHost 惰性收编为 electron 宿主；别名仅为兼容桥', members: web, missingTypeMembers: [...typeNames].filter(name => !webNames.has(name)), missingPreloadMembers: [...bridgeNames].filter(name => !webNames.has(name)), note: 'constantResult仅表明实现返回常量或空体；须结合消费者判断是否已明确提示，不能据此全称产品失败或通过' },
  mobileFallback: { file: 'desktop/src/react/mobile/mobile-platform.ts', members: mobile, partialCast: 'Partial<PlatformApi> → as PlatformApi；不存在的方法须调用方保护，静态签名不保证能力实际存在' },
  reusableRustServices: rustAdapters,
  serviceTransport: { file: 'desktop/src/react/services/server-connection.ts', candidates: ['requireServerConnection', 'resolveServerConnection', 'appendConnectionAuth', 'buildConnectionUrl', 'buildConnectionWsUrl', 'requestConnectionWsTicket'], mainHttpEntry: 'desktop/src/react/hooks/use-hana-fetch.ts::lingxiFetch/lingxiUrl', currentComposition: 'serviceRoute复用rustSettings/Workbench/Browser/Bridge/Observability/Automation地址转换；业务模块另有真实fetch/WS入口。未形成独立唯一命名ServiceClient并不等于已有传输不可复用', status: 'SOURCE_READ_NOT_RUNTIME_VERIFIED' },
  responsibilityNotes: ['DESKTOP_HOST_CAPABILITY 是规划分类，保留现有系统窗口/通知/选择器/本机更新职责；不是已实施新接口', 'CONNECTION_BOOTSTRAP_TO_SERVICE_CLIENT 应由宿主提供连接信息，由唯一服务客户端持有认证传输和订阅，不让组件判断任务终态', 'MIXED_RESOURCE_IO_REQUIRES_EXECUTION_NODE_SPLIT：本机编辑/查看器宿主I/O与服务端资源/附件需按实际执行节点分开；远端不得悄悄本机执行', 'MIXED_BROWSER_HOST_RELAY_RUST_REMAINS_AUTHORITY：IPC可承载宿主控制/凭证绑定，浏览器实际身份/动作授权仍归Rust；不能新建平行业务内核'],
  notExecuted: ['A03 Web受限能力真实组件结果', 'A04 无final汇总/工具/思考/MOOD真实展开', 'R07准入后的真实Rust服务链路', '生产前端完整分支和运行时动态反射可达性', '正式安装产物及跨平台'],
  boundary,
};
const text = JSON.stringify(result, null, 2) + '\n';
if (process.argv.includes('--check')) {
  if (!fs.existsSync(output) || fs.readFileSync(output, 'utf8') !== text) throw Error('CLIENT_BOUNDARY_PREPARATION_STALE');
  if (boundary.result !== 'PASS') throw Error(`CLIENT_BOUNDARY_VIOLATIONS: ${boundary.productionViolations.length} 处生产直连违例，详见 JSON boundary.productionViolations`);
} else fs.writeFileSync(output, text);
console.log(JSON.stringify({ status: 'PREPARATION_ONLY', boundary: boundary.result, productionViolations: productionViolations.length, testStubAssignments: testStubAssignments.length, testDirectReads: testDirectReads.length, ...result.counts }));
