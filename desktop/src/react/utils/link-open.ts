import { openFilePreview } from './file-preview';
import { extOfName } from './file-kind';
import { currentRustBrowserConnection, openRustBrowserViewer } from '../services/service-client';
import { captureConversationRequestScope } from '../services/conversation-request-scope';
import { showError } from './ui-helpers';
import { usePlatform } from '../hooks/use-platform';

export type LinkTarget =
  | { kind: 'web'; url: string }
  | { kind: 'file'; filePath: string; ext: string; label: string }
  | { kind: 'anchor'; href: string }
  | { kind: 'external'; href: string };

export interface LinkOpenContext {
  baseFilePath?: string | null;
  label?: string | null;
  origin?: 'desk' | 'session';
  sessionPath?: string;
  messageId?: string;
  fileId?: string;
  blockIdx?: number;
}

const EXPLICIT_PROTOCOL_RE = /^[a-zA-Z][a-zA-Z0-9+.-]*:/;
const ABSOLUTE_WINDOWS_PATH_RE = /^[A-Za-z]:[\\/]/;

function normalizePathSeparators(value: string): string {
  return value.replace(/\\/g, '/');
}

function dirnamePortable(filePath: string): string | null {
  const normalized = normalizePathSeparators(filePath);
  const slash = normalized.lastIndexOf('/');
  if (slash < 0) return null;
  if (slash === 0) return '/';
  return normalized.slice(0, slash);
}

function isAbsoluteLocalPath(value: string): boolean {
  return value.startsWith('/') || ABSOLUTE_WINDOWS_PATH_RE.test(value) || value.startsWith('\\\\') || value.startsWith('//');
}

function normalizeJoinedPath(pathname: string): string {
  const normalized = normalizePathSeparators(pathname);
  const prefixMatch = normalized.match(/^(?:[A-Za-z]:|\/\/[^/]+\/[^/]+|\/)?/);
  const prefix = prefixMatch?.[0] ?? '';
  const rest = normalized.slice(prefix.length);
  const parts: string[] = [];

  for (const part of rest.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (parts.length > 0 && parts[parts.length - 1] !== '..') {
        parts.pop();
      } else if (!prefix) {
        parts.push(part);
      }
      continue;
    }
    parts.push(part);
  }

  if (!prefix) return parts.join('/');
  if (prefix.endsWith('/')) return `${prefix}${parts.join('/')}`;
  return parts.length ? `${prefix}/${parts.join('/')}` : prefix;
}

function decodeLinkPath(rawPath: string): string {
  try {
    return decodeURI(rawPath);
  } catch {
    return rawPath;
  }
}

function splitSuffix(raw: string): { pathname: string; suffix: string } {
  const hash = raw.indexOf('#');
  const query = raw.indexOf('?');
  const indexes = [hash, query].filter(index => index >= 0);
  const splitAt = indexes.length ? Math.min(...indexes) : -1;
  if (splitAt < 0) return { pathname: raw, suffix: '' };
  return {
    pathname: raw.slice(0, splitAt),
    suffix: raw.slice(splitAt),
  };
}

function basenamePortable(value: string): string {
  const normalized = normalizePathSeparators(value);
  const slash = normalized.lastIndexOf('/');
  return slash >= 0 ? normalized.slice(slash + 1) : normalized;
}

function fileTarget(filePath: string, label?: string | null): LinkTarget {
  const fallbackLabel = basenamePortable(filePath) || filePath;
  return {
    kind: 'file',
    filePath,
    ext: extOfName(fallbackLabel) || extOfName(filePath) || '',
    label: label?.trim() || fallbackLabel,
  };
}

function fileUrlToPath(href: string): string | null {
  try {
    const url = new URL(href);
    if (url.protocol !== 'file:') return null;
    const decodedPath = decodeURIComponent(url.pathname);
    if (url.host) return normalizeJoinedPath(`//${url.host}${decodedPath}`);
    return normalizeJoinedPath(decodedPath.replace(/^\/([A-Za-z]:\/)/, '$1'));
  } catch {
    return null;
  }
}

function normalizeWebUrl(href: string): string | null {
  try {
    const url = new URL(href);
    if (url.protocol === 'http:' || url.protocol === 'https:') return url.toString();
    return null;
  } catch {
    return null;
  }
}

function hasDesktopBrowserViewer(): boolean {
  return typeof document === 'undefined'
    || document.documentElement.getAttribute('data-platform') !== 'web';
}

export function resolveLinkTarget(href: string, context: LinkOpenContext = {}): LinkTarget {
  const raw = String(href || '').trim();
  if (!raw) return { kind: 'external', href: raw };
  if (raw.startsWith('#')) return { kind: 'anchor', href: raw };

  const webUrl = normalizeWebUrl(raw);
  if (webUrl) return { kind: 'web', url: webUrl };

  if (raw.toLowerCase().startsWith('file:')) {
    const filePath = fileUrlToPath(raw);
    if (filePath) return fileTarget(filePath, context.label);
  }

  const { pathname } = splitSuffix(raw);
  const decodedPath = decodeLinkPath(pathname.trim());
  if (decodedPath && isAbsoluteLocalPath(decodedPath)) {
    return fileTarget(normalizeJoinedPath(decodedPath), context.label);
  }

  if (decodedPath && context.baseFilePath && !EXPLICIT_PROTOCOL_RE.test(decodedPath) && !decodedPath.startsWith('//')) {
    const baseDir = dirnamePortable(context.baseFilePath);
    if (baseDir) return fileTarget(normalizeJoinedPath(`${baseDir}/${decodedPath}`), context.label);
  }

  return { kind: 'external', href: raw };
}

export async function openInternalLink(href: string, context: LinkOpenContext = {}): Promise<boolean> {
  const target = resolveLinkTarget(href, context);
  if (target.kind === 'anchor') return false;

  if (target.kind === 'web') {
    if (hasDesktopBrowserViewer() && typeof usePlatform()?.openBrowserViewer === 'function') {
      if (currentRustBrowserConnection()) {
        // 必须使用链接所属的明确会话，不从当前焦点猜测远端网页归属。
        const scope = captureConversationRequestScope();
        try {
          await openRustBrowserViewer(context.sessionPath || '', false, target.url);
        } catch (error) {
          if (scope.isCurrent()) showError(error instanceof Error ? error.message : String(error));
          return false;
        } finally { scope.dispose(); }
      } else {
        await usePlatform().openBrowserViewer(context.sessionPath
          ? { url: target.url, sessionPath: context.sessionPath }
          : target.url);
      }
    } else {
      usePlatform()?.openExternal?.(target.url);
    }
    return true;
  }

  if (target.kind === 'file') {
    await openFilePreview(target.filePath, target.label, target.ext, {
      origin: context.origin || 'desk',
      sessionPath: context.sessionPath,
      messageId: context.messageId,
      fileId: context.fileId,
      blockIdx: context.blockIdx,
    });
    return true;
  }

  usePlatform()?.openExternal?.(target.href);
  return true;
}

export function openExternalLink(href: string, context: LinkOpenContext = {}): boolean {
  const target = resolveLinkTarget(href, context);
  if (target.kind === 'anchor') return false;
  if (target.kind === 'file') {
    usePlatform()?.openFile?.(target.filePath);
    return true;
  }
  usePlatform()?.openExternal?.(target.kind === 'web' ? target.url : target.href);
  return true;
}

export function copyValueForLink(href: string, context: LinkOpenContext = {}): string {
  const target = resolveLinkTarget(href, context);
  if (target.kind === 'file') return target.filePath;
  if (target.kind === 'web') return target.url;
  return target.href;
}
