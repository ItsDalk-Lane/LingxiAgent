/** 知识库域：/api/knowledge/* 与 Rust /lingxi/v1/knowledge/* 路径逐条同构，仅做前缀转换。
 * Rust 侧路由见 rust/crates/lingxi-service/src/knowledge.rs knowledge_routes()；
 * 语义裁决（权限、导入边界、快照归属）全部在 Rust，本函数不改写参数与 query。 */
export function rustKnowledgeRoute(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  if (/^\/api\/knowledge(?:[/?]|$)/.test(path)) {
    return `/lingxi/v1${path.slice('/api'.length)}`;
  }
  return path;
}
