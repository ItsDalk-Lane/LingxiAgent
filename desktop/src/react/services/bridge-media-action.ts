// 同一次未确认的操作在关闭面板后仍保留编号；连接身份只保留在内存中。
const actions = new Map<string, Map<string, string>>();

export function bridgeMediaRequestId(action: string, fileId: string): string {
  let files = actions.get(action);
  if (!files) {
    if (actions.size >= 128) throw new Error('未确认的发送较多，请先检查已有发送记录');
    files = new Map(); actions.set(action, files);
  }
  const existing = files.get(fileId);
  if (existing) return existing;
  const id = `manual-send-media-${crypto.randomUUID()}`;
  files.set(fileId, id); return id;
}

export function finishBridgeMediaAction(action: string): void { actions.delete(action); }

export function bridgeMediaOutcome(value: Record<string, any>, native: boolean): 'confirmed' | 'pending' {
  if (native) {
    if (value.status === 'confirmed') {
      const ids = value.receipt?.messageIds;
      if (!Array.isArray(ids) || !ids.length || ids.some(id => typeof id !== 'string' || !id)) throw new Error('平台未返回有效的文件发送凭据');
      return 'confirmed';
    }
    if (value.status === 'queued' || value.status === 'started') return 'pending';
    if (value.status === 'unknown') throw new Error('平台尚未确认发送结果，请先查看原消息；重试会查询同一请求');
    throw new Error(value.error || '平台尚未确认文件发送');
  }
  if (value.ok === true) return 'confirmed';
  throw new Error(value.error || '文件发送结果无效');
}
