import type { PreviewItem } from '../types';
import { useStore } from '../stores';
import { resolveServerConnection, appendConnectionAuth, buildConnectionUrl } from '../services/server-connection';
import { captureSessionConnection, ownsSessionConnection, subscribeSessionConnection } from '../services/session-connection-owner';

/** 预览从当前 Rust 连接真实读取，旧窗口的迟到正文不能进入新连接的预览池。 */
async function readRustPreview(filePath: string, previewType: string): Promise<PreviewReadResult | null | undefined> {
  const connection = resolveServerConnection(useStore.getState());
  if (connection?.serverNodeKind !== 'lingxi-service') return undefined;
  if (previewType === 'file-info') return {content: ''};
  const owner = captureSessionConnection();
  const route = previewType === 'docx' ? 'docx-html' : previewType === 'xlsx' ? 'xlsx-html' : BINARY_PREVIEW_TYPES.has(previewType) ? 'read-base64' : 'read';
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 30_000);
  const stop = subscribeSessionConnection(() => { if (!ownsSessionConnection(owner)) controller.abort(); });
  try {
  const response = await fetch(buildConnectionUrl(connection, `/lingxi/v1/fs/${route}?path=${encodeURIComponent(filePath)}`), {headers: appendConnectionAuth(connection), signal: controller.signal});
  if (!ownsSessionConnection(owner)) throw new Error('连接已切换，旧文件预览已丢弃');
  if (!response.ok) throw new Error(`文件预览读取失败（${response.status}）`);
  const maxBytes = 32 * 1024 * 1024;
  if (Number(response.headers.get('content-length')) > maxBytes) throw new Error('预览内容过大');
  const reader = response.body?.getReader();
  if (!reader) throw new Error('文件预览正文缺失');
  const decoder = new TextDecoder(); let bytes = 0; let content = '';
  try {
    for (;;) {
      const chunk = await reader.read(); if (chunk.done) break;
      bytes += chunk.value.byteLength;
      if (bytes > maxBytes) throw new Error('预览内容过大');
      content += decoder.decode(chunk.value, {stream: true});
    }
    content += decoder.decode();
  } finally { await reader.cancel().catch(() => {}); }
  if (!ownsSessionConnection(owner)) throw new Error('连接已切换，旧文件预览正文已丢弃');
  if (route === 'read' && content.includes('\0')) throw new Error('二进制文件不能按文本预览');
  return {content};
  } finally { clearTimeout(timeout); stop(); }
}


export const PREVIEWABLE_EXTS: Record<string, string> = {
  // 富预览类型（专属渲染管线）
  html: 'html', htm: 'html', xhtml: 'html',
  md: 'markdown', markdown: 'markdown', mdx: 'markdown',
  csv: 'csv', pdf: 'pdf',
  docx: 'docx', xlsx: 'xlsx', xls: 'xlsx',

  // 以下为纯文本 / 代码类，与 VS Code 一样按可编辑文本打开。
  // 表外扩展名不会被拒绝——openFilePreview / openRemoteContentPreview 会
  // 尝试按文本读取并做二进制嗅探，读得出来照样以 code 打开。
  // Web / JS 生态
  js: 'code', mjs: 'code', cjs: 'code', jsx: 'code',
  ts: 'code', mts: 'code', cts: 'code', tsx: 'code',
  json: 'code', jsonc: 'code', json5: 'code', jsonl: 'code', ndjson: 'code', map: 'code',
  vue: 'code', svelte: 'code', astro: 'code',
  css: 'code', scss: 'code', sass: 'code', less: 'code', styl: 'code', stylus: 'code',
  // 标记 / 模板
  xml: 'code', xsl: 'code', xslt: 'code', dtd: 'code', plist: 'code', rss: 'code', atom: 'code',
  ejs: 'code', hbs: 'code', handlebars: 'code', mustache: 'code', twig: 'code',
  liquid: 'code', njk: 'code', jinja: 'code', jinja2: 'code', erb: 'code',
  slim: 'code', haml: 'code', pug: 'code', jade: 'code', tpl: 'code',
  cshtml: 'code', vbhtml: 'code', razor: 'code', aspx: 'code', asp: 'code', jsp: 'code',
  // 配置 / 数据
  yaml: 'code', yml: 'code', toml: 'code', ini: 'code', cfg: 'code', conf: 'code',
  config: 'code', properties: 'code', env: 'code', editorconfig: 'code',
  gitignore: 'code', gitattributes: 'code', gitmodules: 'code', gitconfig: 'code',
  dockerignore: 'code', npmignore: 'code', npmrc: 'code', yarnrc: 'code', nvmrc: 'code',
  eslintrc: 'code', prettierrc: 'code', babelrc: 'code', browserslistrc: 'code',
  lock: 'code', ipynb: 'code', tsv: 'code', tab: 'code', psv: 'code',
  reg: 'code', desktop: 'code', service: 'code', timer: 'code', socket: 'code',
  pem: 'code', crt: 'code', cer: 'code', key: 'code', pub: 'code',
  po: 'code', pot: 'code',
  // 文档 / 日志
  txt: 'code', text: 'code', log: 'code', rst: 'code', adoc: 'code', asciidoc: 'code',
  org: 'code', tex: 'code', latex: 'code', bib: 'code', sty: 'code', cls: 'code',
  me: 'code', man: 'code', pod: 'code',
  // 系统编程
  c: 'code', h: 'code', cc: 'code', cpp: 'code', cxx: 'code', hpp: 'code', hh: 'code', hxx: 'code',
  cu: 'code', cuh: 'code', m: 'code', mm: 'code',
  cs: 'code', vb: 'code', fs: 'code', fsx: 'code', fsi: 'code',
  java: 'code', kt: 'code', kts: 'code', scala: 'code', sc: 'code', groovy: 'code', gradle: 'code',
  rs: 'code', go: 'code', zig: 'code', nim: 'code', v: 'code', sv: 'code', vh: 'code',
  d: 'code', pas: 'code', pp: 'code', f: 'code', f90: 'code', f95: 'code', for: 'code',
  cob: 'code', cbl: 'code', ada: 'code', adb: 'code', ads: 'code',
  asm: 'code', s: 'code', wat: 'code', wast: 'code',
  glsl: 'code', frag: 'code', vert: 'code', geom: 'code', comp: 'code',
  shader: 'code', hlsl: 'code', wgsl: 'code', metal: 'code',
  // 脚本 / 函数式
  py: 'code', pyi: 'code', pyx: 'code', pxd: 'code', ipy: 'code',
  rb: 'code', php: 'code', lua: 'code', pl: 'code', pm: 'code', t: 'code',
  sh: 'code', bash: 'code', zsh: 'code', fish: 'code', nu: 'code', awk: 'code', sed: 'code',
  ps1: 'code', psm1: 'code', bat: 'code', cmd: 'code', tcl: 'code', vim: 'code',
  r: 'code', jl: 'code', hs: 'code', lhs: 'code', ml: 'code', mli: 'code',
  ex: 'code', exs: 'code', erl: 'code', hrl: 'code',
  clj: 'code', cljs: 'code', cljc: 'code', edn: 'code', lisp: 'code', lsp: 'code',
  el: 'code', scm: 'code', ss: 'code',
  coffee: 'code', litcoffee: 'code', hx: 'code', purs: 'code',
  re: 'code', rei: 'code', res: 'code', resi: 'code', qml: 'code',
  // 查询 / 合约 / 基础设施
  sql: 'code', mysql: 'code', pgsql: 'code', cypher: 'code', sparql: 'code',
  graphql: 'code', gql: 'code', proto: 'code', thrift: 'code', capnp: 'code',
  prisma: 'code', sol: 'code', move: 'code', cairo: 'code',
  tf: 'code', tfvars: 'code', hcl: 'code', nix: 'code', dhall: 'code',
  cue: 'code', rego: 'code', bzl: 'code', bazel: 'code', star: 'code',
  mk: 'code', make: 'code', cmake: 'code', ninja: 'code', meson: 'code',
  // 图形 / 补丁
  diff: 'code', patch: 'code',
  mermaid: 'code', puml: 'code', plantuml: 'code', dot: 'code', gv: 'code',
};

export const BINARY_PREVIEW_TYPES = new Set(['pdf']);

export interface PreviewReadResult {
  content: string;
  sourceUrl?: string;
  fileVersion?: PreviewItem['fileVersion'];
}

export async function readFileForPreviewType(filePath: string, previewType: string): Promise<PreviewReadResult | null> {
  const rust = await readRustPreview(filePath, previewType);
  if (rust !== undefined) return rust;
  const p = window.platform;
  if (!p) return null;
  if (previewType === 'file-info') return { content: '' };
  if (previewType === 'docx') {
    const content = await p.readDocxHtml?.(filePath);
    return content == null ? null : { content };
  }
  if (previewType === 'xlsx') {
    const content = await p.readXlsxHtml?.(filePath);
    return content == null ? null : { content };
  }
  if (BINARY_PREVIEW_TYPES.has(previewType)) {
    const sourceUrl = p.getFileUrl?.(filePath);
    if (sourceUrl) return { content: '', sourceUrl };
    const content = await p.readFileBase64?.(filePath);
    return content == null ? null : { content };
  }

  const snapshot = await p.readFileSnapshot?.(filePath);
  if (snapshot) return { content: snapshot.content, fileVersion: snapshot.version };

  const content = await p.readFile?.(filePath);
  return content == null ? null : { content };
}

export async function readFileForPreviewWithVersion(filePath: string, ext: string): Promise<PreviewReadResult | null> {
  const normalizedExt = ext.replace(/^\./, '').toLowerCase();
  const previewType = PREVIEWABLE_EXTS[normalizedExt];
  if (!previewType) return null;
  return readFileForPreviewType(filePath, previewType);
}

export async function readFileForPreview(filePath: string, ext: string): Promise<string | null> {
  return (await readFileForPreviewWithVersion(filePath, ext))?.content ?? null;
}
