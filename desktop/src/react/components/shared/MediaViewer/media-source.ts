import type { FileRef } from '../../../types/file-ref';
import { useStore } from '../../../stores';
import { resolveServerConnection } from '../../../services/service-client';
import { resolveFileRefUrl } from '../../../services/resource-url';
import { usePlatform } from '../../../hooks/use-platform';

export interface MediaSource {
  url: string;
  cleanup?: () => void;
}

/**
 * FileRef → 可供 <img> / <video> 直接消费的 URL。
 *
 * 设计原则：
 *   - 优先使用受控资源地址，本地服务器也可能搭配不能读取 file:// 的网页界面。
 *   - 无资源地址的本地文件沿用 platform.getFileUrl，远程文件不得用客户端路径代替。
 *   - 只有无 path 且无 resource content link 的 inline 数据才走 data URL。
 */
export async function loadMediaSource(ref: FileRef): Promise<MediaSource> {
  // R08-T02：经宿主边界取平台能力；Web 宿主下 getFileUrl 缺席时 resolveFileRefUrl 走既有资源地址分支。
  const platform = usePlatform();

  if (ref.kind !== 'image' && ref.kind !== 'svg' && ref.kind !== 'video') {
    throw new Error(`unsupported media kind: ${ref.kind}`);
  }

  const connection = resolveServerConnection(useStore.getState());
  const source = resolveFileRefUrl(ref, {
    connection, platform, preferLocalFile: !ref.resource?.links.content,
  });
  return { url: source.url };
}
