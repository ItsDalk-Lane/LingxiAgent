/** 主窗口与设置窗口共用现役设置地址，查询参数和明确的助手身份原样保留。 */
export function rustSettingsRoute(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  if (/^\/api\/config(?:[/?]|$)/.test(path)) {
    return `/lingxi/v1/settings/config${path.slice('/api/config'.length)}`;
  }
  if (/^\/api\/(?:agents|memories|character-cards|cards|skills|experiments|models|providers|user-profile|agent-deletions|speech-recognition)(?:[/?]|$)/.test(path)
    || /^\/api\/media\/(?:image|video|speech)(?:[/?]|$)/.test(path)
    || /^\/api\/preferences\/(?:models|goal|autolearn|computer-use|quick-chat|notifications|keybindings|sidebar-ui|session-permission-default)(?:[/?]|$)/.test(path)
    || /^\/api\/checkpoints(?:[/?]|$)/.test(path)
    || /^\/api\/sessions\/workspace-rollback(?:[?]|$)/.test(path)
    || /^\/api\/settings\/(?:snapshot|export|import)(?:[/?]|$)/.test(path)) {
    return `/lingxi/v1${path.slice('/api'.length)}`;
  }
  if (path === '/api/health') return '/lingxi/v1/settings/avatar-health';
  if (/^\/api\/(?:avatar|access|devices)\//.test(path)) return `/lingxi/v1${path.slice('/api'.length)}`;
  return path;
}
