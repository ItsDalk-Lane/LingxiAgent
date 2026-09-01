import type { UserAttachment } from '../stores/chat-types';
import type { PlatformApi } from '../types';

type FileUrlPlatform = Pick<PlatformApi, 'getFileUrl'> | null | undefined;

export function getUserAttachmentImageSrc(
  attachment: Pick<UserAttachment, 'path' | 'base64Data' | 'mimeType'>,
  platform: FileUrlPlatform = typeof window !== 'undefined' ? window.platform : undefined,
): string | null {
  if (attachment.base64Data) {
    return `data:${attachment.mimeType || 'image/png'};base64,${attachment.base64Data}`;
  }
  if (attachment.path && typeof platform?.getFileUrl === 'function') {
    return platform.getFileUrl(attachment.path);
  }
  return null;
}

/**
 * 用户消息里视频附件的海报帧来源。与图片同源策略：内存 base64 → data URL
 * （<video preload="metadata"> 会渲染首帧）；本地桌面 → platform.getFileUrl。
 * 拿不到源（远程且无 resource）时返回 null，由调用方退回文件胶囊。
 */
export function getUserAttachmentVideoPosterSrc(
  attachment: Pick<UserAttachment, 'path' | 'base64Data' | 'mimeType'>,
  platform: FileUrlPlatform = typeof window !== 'undefined' ? window.platform : undefined,
): string | null {
  if (attachment.base64Data) {
    return `data:${attachment.mimeType || 'video/mp4'};base64,${attachment.base64Data}`;
  }
  if (attachment.path && typeof platform?.getFileUrl === 'function') {
    return platform.getFileUrl(attachment.path);
  }
  return null;
}
