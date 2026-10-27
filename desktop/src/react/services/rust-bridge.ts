/** 仅切换已经接入原生服务的消息平台管理入口，保留查询参数。 */
export function rustBridgeRoute(connection: { serverNodeKind?: string }, path: string): string {
  return connection.serverNodeKind === 'lingxi-service'
    && /^\/api\/bridge\/(?:status|config|owner|settings|stop|test|send-media|messages|sessions(?:\/[^/?]+\/(?:messages|reset))?|deliveries\/[^/?]+|wechat\/(?:qrcode|qrcode-status))(?:\?|$)/.test(path)
    ? `/lingxi/v1${path.slice('/api'.length)}` : path;
}
