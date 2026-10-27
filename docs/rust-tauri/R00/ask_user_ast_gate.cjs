/* R00-T02：从真实 action 来源解析选择分支，无法静态解释时交人工审查。 */
const fs = require('node:fs');
const crypto = require('node:crypto');
const ts = require('typescript');

const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const source = ts.createSourceFile('ask-user-tool.ts', input.source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const printer = ts.createPrinter({ removeComments: true });
function canonical(node) { return printer.printNode(ts.EmitHint.Unspecified, node, source); }
function digest(parts) { return crypto.createHash('sha256').update(parts.join('\n')).digest('hex'); }
const OTHER = Symbol('OTHER');
const UNDEFINED = Symbol('UNDEFINED');
const UNKNOWN = Symbol('UNKNOWN');
const methods = new Map();
const manual = [];
const conditions = [];
let actionDeclaration;
let actionBlock;

function loc(node) {
  const p = source.getLineAndCharacterOfPosition(node.getStart(source));
  return `lib/tools/ask-user-tool.ts:${p.line + 1}:${p.character + 1}`;
}
function unwrap(node) {
  while (node && (ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isNonNullExpression(node))) node = node.expression;
  return node;
}
function isActionRead(node) {
  node = unwrap(node);
  return node && (ts.isPropertyAccessExpression(node) || ts.isPropertyAccessChain(node))
    && node.name.text === 'action' && ts.isIdentifier(node.expression) && node.expression.text === 'decision';
}
function walk(node) {
  if (ts.isFunctionDeclaration(node) && node.name) methods.set(node.name.text, node);
  if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer) {
    const init = unwrap(node.initializer);
    if (ts.isArrowFunction(init) || ts.isFunctionExpression(init)) methods.set(node.name.text, init);
    if (isActionRead(init)) {
      if (actionDeclaration) throw new Error('多个 decision.action 来源，需 MANUAL');
      actionDeclaration = node;
      let parent = node.parent;
      while (parent && !ts.isBlock(parent)) parent = parent.parent;
      actionBlock = parent;
    }
  }
  ts.forEachChild(node, walk);
}
walk(source);
const decisionDeclaration = actionBlock?.statements
  .filter(ts.isVariableStatement)
  .flatMap(stmt => [...stmt.declarationList.declarations])
  .find(declaration => declaration.pos < actionDeclaration?.pos
    && ts.isIdentifier(declaration.name) && declaration.name.text === 'decision'
    && declaration.initializer && ts.isAwaitExpression(declaration.initializer)
    && ts.isIdentifier(declaration.initializer.expression)
    && declaration.initializer.expression.text === 'promise');
if (!actionDeclaration || !actionBlock || !decisionDeclaration) {
  throw new Error('ask_user 的 decision.action 来源已改变，需 MANUAL');
}

function item(value, dep = false, unknown = false) { return { value, dep, unknown }; }
function valueOf(node, env, depth = 0) {
  node = unwrap(node);
  if (!node || depth > 12) return item(UNKNOWN, true, true);
  if (ts.isStringLiteralLike(node)) return item(node.text);
  if (node.kind === ts.SyntaxKind.TrueKeyword) return item(true);
  if (node.kind === ts.SyntaxKind.FalseKeyword) return item(false);
  if (node.kind === ts.SyntaxKind.NullKeyword) return item(null);
  if (ts.isIdentifier(node)) return env.get(node.text) ?? item(UNKNOWN, false, true);
  if (isActionRead(node)) return env.get('$action') ?? item(UNKNOWN, true, true);
  if (ts.isPropertyAccessExpression(node) || ts.isPropertyAccessChain(node)) {
    const base = valueOf(node.expression, env, depth + 1);
    return item(UNKNOWN, base.dep, true);
  }
  if (ts.isPrefixUnaryExpression(node) && node.operator === ts.SyntaxKind.ExclamationToken) {
    const a = valueOf(node.operand, env, depth + 1);
    return item(a.unknown ? UNKNOWN : !a.value, a.dep, a.unknown);
  }
  if (ts.isBinaryExpression(node)) {
    const left = valueOf(node.left, env, depth + 1);
    const right = valueOf(node.right, env, depth + 1);
    const dep = left.dep || right.dep;
    const op = node.operatorToken.kind;
    if (op === ts.SyntaxKind.AmpersandAmpersandToken || op === ts.SyntaxKind.BarBarToken) {
      if (!left.unknown && ((op === ts.SyntaxKind.AmpersandAmpersandToken && !left.value) || (op === ts.SyntaxKind.BarBarToken && left.value))) return item(Boolean(left.value), left.dep);
      return item(left.unknown || right.unknown ? UNKNOWN : Boolean(right.value), dep, left.unknown || right.unknown);
    }
    if (left.unknown || right.unknown) return item(UNKNOWN, dep, true);
    if (op === ts.SyntaxKind.EqualsEqualsEqualsToken) return item(left.value === right.value, dep);
    if (op === ts.SyntaxKind.ExclamationEqualsEqualsToken) return item(left.value !== right.value, dep);
    return item(UNKNOWN, dep, true);
  }
  if (ts.isConditionalExpression(node)) {
    const test = valueOf(node.condition, env, depth + 1);
    if (test.unknown) return item(UNKNOWN, test.dep, true);
    const branch = valueOf(test.value ? node.whenTrue : node.whenFalse, env, depth + 1);
    return item(branch.value, branch.dep || test.dep, branch.unknown);
  }
  if (ts.isArrayLiteralExpression(node)) {
    const elements = node.elements.map(x => valueOf(x, env, depth + 1));
    return item(elements.some(x => x.unknown) ? UNKNOWN : elements.map(x => x.value), elements.some(x => x.dep), elements.some(x => x.unknown));
  }
  if (ts.isObjectLiteralExpression(node)) {
    const values = node.properties.map(prop => ts.isPropertyAssignment(prop)
      ? valueOf(prop.initializer, env, depth + 1)
      : item(UNKNOWN, false, true));
    return item(UNKNOWN, values.some(x => x.dep), true);
  }
  if (ts.isNewExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'Set') {
    const values = node.arguments?.[0] ? valueOf(node.arguments[0], env, depth + 1) : item([]);
    return item(values.unknown || !Array.isArray(values.value) ? UNKNOWN : new Set(values.value), values.dep, values.unknown || !Array.isArray(values.value));
  }
  if (ts.isCallExpression(node)) {
    const callee = unwrap(node.expression);
    if (ts.isPropertyAccessExpression(callee) && ['includes', 'has'].includes(callee.name.text) && node.arguments.length === 1) {
      const collection = valueOf(callee.expression, env, depth + 1);
      const searched = valueOf(node.arguments[0], env, depth + 1);
      const dep = collection.dep || searched.dep;
      if (collection.unknown || searched.unknown) return item(UNKNOWN, dep, true);
      if (callee.name.text === 'includes' && Array.isArray(collection.value)) return item(collection.value.includes(searched.value), dep);
      if (callee.name.text === 'has' && collection.value instanceof Set) return item(collection.value.has(searched.value), dep);
      return item(UNKNOWN, dep, true);
    }
    if (ts.isIdentifier(callee) && methods.has(callee.text)) {
      const method = methods.get(callee.text);
      const args = node.arguments.map(x => valueOf(x, env, depth + 1));
      const dep = args.some(x => x.dep);
      if (args.some(x => x.unknown) || method.parameters.length !== args.length) return item(UNKNOWN, dep, true);
      const local = new Map(env);
      method.parameters.forEach((parameter, i) => {
        if (!ts.isIdentifier(parameter.name)) return;
        local.set(parameter.name.text, args[i]);
      });
      let returned;
      if (ts.isBlock(method.body)) {
        for (const stmt of method.body.statements) {
          if (ts.isVariableStatement(stmt)) assign(stmt, local, depth + 1);
          else if (ts.isReturnStatement(stmt)) { returned = stmt.expression; break; }
          else return item(UNKNOWN, dep, true);
        }
      } else returned = method.body;
      if (!returned) return item(UNKNOWN, dep, true);
      const result = valueOf(returned, local, depth + 1);
      return item(result.value, result.dep || dep, result.unknown);
    }
  }
  let dep = false;
  ts.forEachChild(node, child => { if (valueOf(child, env, depth + 1).dep) dep = true; });
  return item(UNKNOWN, dep, true);
}
function assign(statement, env, depth = 0) {
  for (const declaration of statement.declarationList.declarations) {
    if (ts.isIdentifier(declaration.name)) env.set(declaration.name.text, declaration.initializer ? valueOf(declaration.initializer, env, depth + 1) : item(UNDEFINED));
    else if (ts.isObjectBindingPattern(declaration.name) && declaration.initializer) {
      const base = valueOf(declaration.initializer, env, depth + 1);
      for (const element of declaration.name.elements) if (ts.isIdentifier(element.name)) {
        const key = element.propertyName?.getText(source) ?? element.name.text;
        env.set(element.name.text, key === 'action' && base.dep ? item(base.value, true, base.unknown) : item(UNKNOWN, base.dep, true));
      }
    }
  }
}
function hasActionDependency(node, env) { return valueOf(node, env).dep; }
function literals(node, out = new Set()) {
  if (ts.isStringLiteralLike(node)) out.add(node.text);
  ts.forEachChild(node, x => { literals(x, out); });
  return out;
}
function candidateLiterals(node, env, out = new Set(), visited = new Set()) {
  if (hasActionDependency(node, env)) {
    for (const lit of literals(node)) out.add(lit);
    const visit = child => {
      if (ts.isIdentifier(child)) {
        const stored = env.get(child.text)?.value;
        const members = stored instanceof Set ? [...stored] : Array.isArray(stored) ? stored : [];
        for (const member of members) if (typeof member === 'string') out.add(member);
        if (typeof stored === 'string') out.add(stored);
      }
      if (ts.isCallExpression(child) && ts.isIdentifier(child.expression) && methods.has(child.expression.text) && !visited.has(child.expression.text)) {
        visited.add(child.expression.text);
        const method = methods.get(child.expression.text);
        if (method.body) for (const lit of literals(method.body)) out.add(lit);
      }
      ts.forEachChild(child, visit);
    };
    visit(node);
  }
  return out;
}
function signature(expr, env) {
  expr = unwrap(expr);
  if (!expr || !ts.isCallExpression(expr) || !ts.isIdentifier(expr.expression) || !['toolOk', 'toolError'].includes(expr.expression.text)) return 'NO_TOOL_RESULT';
  const flags = [];
  for (const arg of expr.arguments) if (ts.isObjectLiteralExpression(arg)) {
    for (const prop of arg.properties) if (ts.isPropertyAssignment(prop) && ['answered', 'timedOut', 'aborted', 'dismissed'].includes(prop.name.getText(source))) {
      flags.push(`${prop.name.getText(source)}=${prop.initializer.getText(source)}`);
    }
  }
  if (expr.arguments.some(arg => hasActionDependency(arg, env))) manual.push({ location: loc(expr), reason: 'action 直接影响工具结果，需逐值人工裁决' });
  return `${expr.expression.text}:${flags.join(',')}`;
}
function evalStatements(statements, env, trace, depth = 0, resultParts = []) {
  if (depth > 30) return { signature: 'MANUAL', trace };
  for (const statement of statements) {
    if (ts.isVariableStatement(statement)) { assign(statement, env, depth); resultParts.push(canonical(statement)); continue; }
    if (ts.isIfStatement(statement)) {
      const test = valueOf(statement.expression, env, depth + 1);
      if (test.dep) conditions.push({ location: loc(statement.expression), expression: statement.expression.getText(source), normalized: canonical(statement.expression) });
      if (test.unknown) {
        if (test.dep) {
          manual.push({ location: loc(statement.expression), expression: statement.expression.getText(source), reason: 'action 相关条件无法静态求值' });
          return { signature: 'MANUAL', trace };
        }
        const yes = evalStatements(ts.isBlock(statement.thenStatement) ? statement.thenStatement.statements : [statement.thenStatement], new Map(env), trace, depth + 1, [...resultParts]);
        const no = statement.elseStatement
          ? evalStatements(ts.isBlock(statement.elseStatement) ? statement.elseStatement.statements : [statement.elseStatement], new Map(env), trace, depth + 1, [...resultParts])
          : { signature: null, trace };
        if (yes.signature !== no.signature || yes.digest !== no.digest) {
          manual.push({ location: loc(statement.expression), expression: statement.expression.getText(source), reason: '未知非 action 条件改变工具结果' });
          return { signature: 'MANUAL', trace };
        }
        if (yes.signature) return yes;
        continue;
      }
      const taken = test.value ? statement.thenStatement : statement.elseStatement;
      if (taken) {
        const body = ts.isBlock(taken) ? taken.statements : [taken];
        const result = evalStatements(body, new Map(env), [...trace, `${loc(statement.expression)}=${Boolean(test.value)}`], depth + 1, [...resultParts]);
        if (result.signature) return result;
      }
      continue;
    }
    if (ts.isSwitchStatement(statement)) {
      const selector = valueOf(statement.expression, env, depth + 1);
      if (selector.dep) conditions.push({ location: loc(statement.expression), expression: statement.expression.getText(source), normalized: canonical(statement.expression) });
      if (selector.unknown) { manual.push({ location: loc(statement.expression), reason: 'action 相关 switch 无法静态求值' }); return { signature: 'MANUAL', trace }; }
      if (statement.caseBlock.clauses.some(clause => ts.isCaseClause(clause) && valueOf(clause.expression, env).unknown)) {
        manual.push({ location: loc(statement.expression), reason: 'switch case 值无法静态求值' });
        return { signature: 'MANUAL', trace };
      }
      const matched = statement.caseBlock.clauses.find(clause =>
        ts.isCaseClause(clause) && valueOf(clause.expression, env).value === selector.value);
      const entry = matched || statement.caseBlock.clauses.find(ts.isDefaultClause);
      let found = false;
      for (const clause of statement.caseBlock.clauses) {
        if (clause === entry) found = true;
        if (found) {
          const result = evalStatements(clause.statements, new Map(env), [...trace, loc(clause)], depth + 1, [...resultParts]);
          if (result.signature) return result;
        }
      }
      continue;
    }
    if (ts.isReturnStatement(statement)) return { signature: signature(statement.expression, env), trace: [...trace, loc(statement)], digest: digest([...resultParts, canonical(statement)]) };
    if (ts.isThrowStatement(statement)) return { signature: 'THROW', trace: [...trace, loc(statement)], digest: digest([...resultParts, canonical(statement)]) };
    if (ts.isBlock(statement)) {
      const result = evalStatements(statement.statements, new Map(env), trace, depth + 1, [...resultParts]);
      if (result.signature) return result;
      continue;
    }
    if (ts.isExpressionStatement(statement)) {
      const expr = unwrap(statement.expression);
      if (ts.isCallExpression(expr) && ts.isPropertyAccessExpression(expr.expression)
          && ts.isIdentifier(expr.expression.expression)
          && ['add', 'push', 'delete', 'clear', 'splice', 'pop', 'shift', 'unshift'].includes(expr.expression.name.text)) {
        const collectionName = expr.expression.expression.text;
        if (env.has(collectionName)) env.set(collectionName, item(UNKNOWN, false, true));
      }
      if (hasActionDependency(statement.expression, env)) manual.push({ location: loc(statement), reason: 'action 影响副作用表达式，需人工裁决' });
      continue;
    }
    manual.push({ location: loc(statement), reason: '结果路径存在未支持语句，需人工裁决' });
    return { signature: 'MANUAL', trace };
  }
  return { signature: null, trace };
}

const start = actionBlock.statements.findIndex(stmt => stmt.pos <= actionDeclaration.pos && stmt.end >= actionDeclaration.end);
const baseline = new Map();
for (const stmt of actionBlock.statements.slice(0, start + 1)) if (ts.isVariableStatement(stmt)) assign(stmt, baseline);
const actionName = actionDeclaration.name.text;
const proposed = new Set(input.contractValues.filter(x => x !== '<default>'));
const scanEnv = new Map([[actionName, item(OTHER, true)], ['$action', item(OTHER, true)]]);
function scanStatements(statements, env) {
  for (const stmt of statements) {
    if (ts.isVariableStatement(stmt)) {
      for (const declaration of stmt.declarationList.declarations) if (declaration.initializer) candidateLiterals(declaration.initializer, env, proposed);
      assign(stmt, env);
    } else if (ts.isIfStatement(stmt)) {
      candidateLiterals(stmt.expression, env, proposed);
      scanStatements(ts.isBlock(stmt.thenStatement) ? stmt.thenStatement.statements : [stmt.thenStatement], new Map(env));
      if (stmt.elseStatement) scanStatements(ts.isBlock(stmt.elseStatement) ? stmt.elseStatement.statements : [stmt.elseStatement], new Map(env));
    } else if (ts.isSwitchStatement(stmt) && hasActionDependency(stmt.expression, env)) {
      for (const clause of stmt.caseBlock.clauses) if (ts.isCaseClause(clause)) for (const lit of literals(clause.expression)) proposed.add(lit);
      for (const clause of stmt.caseBlock.clauses) scanStatements(clause.statements, new Map(env));
    } else if (ts.isBlock(stmt)) scanStatements(stmt.statements, new Map(env));
  }
}
scanStatements(actionBlock.statements.slice(start + 1), scanEnv);
const results = {};
const initial = actionBlock.statements.slice(start + 1);
for (const value of [...proposed, 'rejected', OTHER, null, UNDEFINED]) {
  const env = new Map(baseline);
  env.set(actionName, item(value, true));
  env.set('$action', item(value, true));
  const result = evalStatements(initial, env, []);
  const key = value === OTHER ? '<default>' : value === null ? '<null>' : value === UNDEFINED ? '<undefined>' : value;
  results[key] = result;
}
const defaultSignature = results['<default>']?.signature;
const branches = {};
for (const value of proposed) branches[value] = results[value]?.signature;
branches['<default>'] = defaultSignature;
for (const [key, result] of Object.entries(results)) if (!result.signature || result.signature === 'MANUAL') manual.push({ value: key, reason: '选择路径没有可确定的工具结果', trace: result.trace });
for (const key of ['<null>', '<undefined>', 'rejected']) if (results[key].signature !== defaultSignature) manual.push({ value: key, reason: '兜底域结果与 OTHER 不同，须拆分契约' });
const uniqueConditions = [...new Map(conditions.map(x => [x.location + x.expression, x])).values()];
process.stdout.write(JSON.stringify({ branches, results, conditions: uniqueConditions,
  condition_digest: digest(uniqueConditions.map(x => x.normalized).sort()),
  manual: [...new Map(manual.map(x => [JSON.stringify(x), x])).values()] }));
