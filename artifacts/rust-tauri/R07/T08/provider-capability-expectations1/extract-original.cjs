// 只解析原 TypeScript 声明，不执行供应商模块，不读取 Rust summary 输出。
const ts = require('typescript');
const fs = require('node:fs');
const crypto = require('node:crypto');
const path = require('node:path');
const root = process.cwd();
const out = [];
for (const file of fs.readdirSync('lib/providers').filter(f => f.endsWith('.ts'))) {
  const relative = 'lib/providers/' + file;
  const text = fs.readFileSync(relative, 'utf8');
  const ast = ts.createSourceFile(relative, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const variables = new Map();
  function visit(n) {
    if (ts.isVariableDeclaration(n) && ts.isIdentifier(n.name) && n.initializer) variables.set(n.name.text, n.initializer);
    ts.forEachChild(n, visit);
  }
  visit(ast);
  const resolve = n => n && ts.isIdentifier(n) ? variables.get(n.text) : n;
  const prop = (n, key) => { n = resolve(n); return n && ts.isObjectLiteralExpression(n) ? n.properties.find(p => p.name && p.name.getText(ast).replace(/^['"]|['"]$/g, '') === key) : undefined; };
  const value = (n, key) => prop(n, key)?.initializer;
  const string = n => n && ts.isStringLiteral(n) ? n.text : undefined;
  const cite = n => ({ path: relative, line: ast.getLineAndCharacterOfPosition(n.getStart(ast)).line + 1, endLine: ast.getLineAndCharacterOfPosition(n.getEnd()).line + 1, text: n.getText(ast) });
  for (const [name, object] of variables) {
    if (!ts.isObjectLiteralExpression(object)) continue;
    const id = string(value(object, 'id'));
    if (!id || !string(value(object, 'authType'))) continue;
    const media = resolve(value(resolve(value(object, 'capabilities')), 'media'));
    const capabilities = [];
    if (media && ts.isObjectLiteralExpression(media)) for (const property of media.properties) {
      const capability = property.name?.getText(ast).replace(/^['"]|['"]$/g, '');
      const block = resolve(property.initializer);
      if (!['imageGeneration','videoGeneration','speechRecognition','speechGeneration'].includes(capability)) continue;
      const lanes = value(block, 'credentialLanes');
      const models = value(block, 'models');
      const citations = [cite(property)];
      if (block && block !== property.initializer) citations.push(cite(block));
      capabilities.push({ capability, declaration: citations, credentialLanes: lanes && ts.isArrayLiteralExpression(lanes) ? lanes.elements.map(l => ({id: string(value(l,'id')), providerId: string(value(l,'providerId')), source: cite(l)})) : [], explicitLanes: !!lanes, modelProtocols: models && ts.isArrayLiteralExpression(models) ? [...new Set(models.elements.map(m => string(value(m,'protocolId'))).filter(Boolean))] : [] });
    }
    out.push({ id, exportName: name, file: relative, sha256: crypto.createHash('sha256').update(text).digest('hex'), source: cite(object), capabilities });
  }
}
fs.writeFileSync(path.join(__dirname, 'original-provider-declarations.json'), JSON.stringify(out, null, 2) + '\n');
process.stdout.write(JSON.stringify({ providers: out.length, withMedia: out.filter(r => r.capabilities.length).map(r => [r.id,r.capabilities.map(c => c.capability)]) }) + '\n');
