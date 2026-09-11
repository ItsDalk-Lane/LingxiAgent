import type { AudioWaveform } from './chat-types';
import type { JSONContent } from '@tiptap/core';
import { sessionScopedKey } from './session-slice';
import { notifyDraftCleared, notifyDraftSet } from './input-draft-sync';
import { HOME_DRAFT_KEY } from '../../../../shared/input-drafts.ts';

export interface AttachedFile {
  fileId?: string;
  path: string;
  name: string;
  isDirectory?: boolean;
  /** 内联 base64 数据（粘贴图片时使用，跳过文件读取） */
  base64Data?: string;
  mimeType?: string;
  waveform?: AudioWaveform;
}

export interface DocContextFile {
  path: string;
  name: string;
}

export interface FloatingAnchorRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
  width: number;
  height: number;
}

export interface QuotedSelection {
  text: string;
  sourceTitle: string;
  sourceKind: 'preview' | 'chat';
  sourceFilePath?: string;
  sourceSessionPath?: string;
  sourceMessageId?: string;
  sourceRole?: 'user' | 'assistant';
  lineStart?: number;
  lineEnd?: number;
  selectionAnchorKind?: 'native' | 'codemirror';
  charCount: number;
  anchorRect?: FloatingAnchorRect;
  updatedAt?: number;
}

export interface InputSlice {
  attachedFiles: AttachedFile[];
  /** 按 session path 存储的附件（权威源） */
  attachedFilesBySession: Record<string, AttachedFile[]>;
  /** 按 session path 存储的草稿文本（内存级，关窗口清空） */
  drafts: Record<string, string>;
  /** 按 session path 存储的输入框富文本草稿（内存级，关窗口清空） */
  draftDocs: Record<string, JSONContent>;
  /**
   * 输入区修订号（按草稿 key）：每次有效编辑（文本变化/附件增删/引用增删/
   * 文档上下文开关）递增。发送清理只作用于「点击时修订号未变」的原草稿，
   * 避免准备期间用户继续输入的内容被误清（F1/P2.4）。
   */
  composerRevisionsByKey: Record<string, number>;
  /** 草稿持久化 hydrate 完成时间戳（0 = 未 hydrate）；InputArea 恢复 effect 依赖它重跑 */
  draftsHydratedAt: number;
  deskContextAttached: boolean;
  docContextAttached: boolean;
  /**
   * 按 session path 存储的「文档上下文」开关（权威源）。
   * 侧边会话（/side-chat 面板）与主会话同时挂载时，两个输入区必须各自
   * 记住自己的开关；全局字段保留为「主会话兼容镜像」。
   */
  docContextAttachedBySession: Record<string, boolean>;
  inputFocusTrigger: number;
  /** Source of the most recent requestInputFocus() call; consumers gate 'restore' by surface. */
  inputFocusTriggerSource: 'gesture' | 'restore';
  quoteCandidate: QuotedSelection | null;
  quotedSelections: QuotedSelection[];
  /**
   * 按 session path 存储的已提交引用（权威源）。
   * 与 attachedFilesBySession 同规则：主会话与侧边会话各持一份，读方按自己的
   * session path 取；全局 quotedSelections 只是主会话兼容镜像。
   */
  quotedSelectionsBySession: Record<string, QuotedSelection[]>;
  /** @deprecated Use quotedSelections for committed quotes and quoteCandidate for transient selection UI. */
  quotedSelection: QuotedSelection | null;
  addAttachedFile: (file: AttachedFile) => void;
  removeAttachedFile: (index: number) => void;
  setAttachedFiles: (files: AttachedFile[]) => void;
  clearAttachedFiles: () => void;
  /** 清理指定 session 的附件；只有目标仍是当前 session 时才同步清空可见附件。 */
  clearAttachedFilesForSession: (sessionPath: string) => void;
  setDraft: (sessionPath: string, text: string, doc?: JSONContent | null) => void;
  clearDraft: (sessionPath: string) => void;
  setDeskContextAttached: (attached: boolean) => void;
  toggleDeskContext: () => void;
  setDocContextAttached: (attached: boolean) => void;
  toggleDocContext: () => void;
  /** 按 session path 写文档上下文开关；同时维护「主会话镜像」与修订号。 */
  setDocContextAttachedForSession: (sessionPath: string, attached: boolean) => void;
  requestInputFocus: (source?: 'gesture' | 'restore') => void;
  setQuoteCandidate: (sel: QuotedSelection) => void;
  clearQuoteCandidate: () => void;
  addQuotedSelection: (sel: QuotedSelection) => void;
  removeQuotedSelection: (index: number) => void;
  clearQuotedSelections: () => void;
  setQuotedSelections: (sels: QuotedSelection[]) => void;
  /** 侧边会话等非当前会话输入区使用：引用只写进目标 session 的桶。 */
  addQuotedSelectionForSession: (sessionPath: string, sel: QuotedSelection) => void;
  removeQuotedSelectionForSession: (sessionPath: string, index: number) => void;
  clearQuotedSelectionsForSession: (sessionPath: string) => void;
  /** @deprecated Use addQuotedSelection or setQuoteCandidate. */
  setQuotedSelection: (sel: QuotedSelection) => void;
  /** @deprecated Use clearQuotedSelections and clearQuoteCandidate. */
  clearQuotedSelection: () => void;
}

function syncCurrentSessionAttachments(state: InputSlice & { currentSessionPath?: string | null }, files: AttachedFile[]) {
  const patch: Partial<InputSlice> & { attachedFilesBySession?: Record<string, AttachedFile[]> } = {
    attachedFiles: files,
  };
  const currentSessionPath = state.currentSessionPath;
  if (currentSessionPath) {
    const key = sessionScopedKey(state as any, currentSessionPath) || currentSessionPath;
    patch.attachedFilesBySession = {
      ...state.attachedFilesBySession,
      [key]: files,
    };
    if (key !== currentSessionPath) delete patch.attachedFilesBySession[currentSessionPath];
  }
  return patch;
}

/** 当前输入区的修订号 key：会话草稿（scoped）或首页 pending 草稿。 */
function currentComposerRevisionKey(
  state: InputSlice & { currentSessionPath?: string | null; pendingNewSession?: boolean },
): string | null {
  const path = typeof state.currentSessionPath === 'string' && state.currentSessionPath
    ? state.currentSessionPath
    : null;
  if (path) return sessionScopedKey(state as any, path) || path;
  return state.pendingNewSession ? HOME_DRAFT_KEY : null;
}

function bumpComposerRevision(
  s: InputSlice & { currentSessionPath?: string | null; pendingNewSession?: boolean },
  key: string | null,
): Partial<InputSlice> {
  if (!key) return {};
  return { composerRevisionsByKey: { ...s.composerRevisionsByKey, [key]: (s.composerRevisionsByKey[key] ?? 0) + 1 } };
}

/** 读某个 session 桶的值（scoped key 优先，sessionPath 兜底）。 */
function sessionBucketValue<T>(s: never, map: Record<string, T>, sessionPath: string): T | undefined {
  const key = sessionScopedKey(s as never, sessionPath) || sessionPath;
  if (Object.prototype.hasOwnProperty.call(map, key)) return map[key];
  return Object.prototype.hasOwnProperty.call(map, sessionPath) ? map[sessionPath] : undefined;
}

/** 写某个 session 桶的值；key 与 sessionPath 不同时清掉别名，避免双份状态。 */
function putSessionBucket<T>(map: Record<string, T>, sessionPath: string, value: T, key: string): Record<string, T> {
  const next = { ...map, [key]: value };
  if (key !== sessionPath) delete next[sessionPath];
  return next;
}

function deleteSessionBucket<T>(map: Record<string, T>, sessionPath: string, key: string): Record<string, T> {
  const next = { ...map };
  delete next[key];
  delete next[sessionPath];
  return next;
}

/**
 * 主会话兼容镜像：只有目标就是「当前主会话」时才回写全局字段。
 * 侧边会话（sessionPath ≠ currentSessionPath 且 ≠ override）写入时不动主会话。
 */
function isPrimarySessionTarget(
  s: InputSlice & { currentSessionPath?: string | null },
  sessionPath: string,
): boolean {
  return !!sessionPath && s.currentSessionPath === sessionPath;
}

type ComposerSessionState = InputSlice & { currentSessionPath?: string | null; pendingNewSession?: boolean };

/** 已提交引用写入当前主会话（全局镜像 + scoped 桶同步，避免两份状态分叉）。 */
function addPrimaryQuotedSelection(s: ComposerSessionState, sel: QuotedSelection): Partial<InputSlice> {
  const quotedSelections = [...s.quotedSelections, sel];
  return {
    quotedSelections,
    quotedSelection: quotedSelections[0] ?? null,
    ...(s.currentSessionPath
      ? { quotedSelectionsBySession: putSessionBucket(s.quotedSelectionsBySession, s.currentSessionPath, quotedSelections, sessionScopedKey(s as never, s.currentSessionPath) || s.currentSessionPath) }
      : {}),
    ...bumpComposerRevision(s as never, currentComposerRevisionKey(s as never)),
  };
}

function removePrimaryQuotedSelection(s: ComposerSessionState, index: number): Partial<InputSlice> {
  const quotedSelections = s.quotedSelections.filter((_, i) => i !== index);
  return {
    quotedSelections,
    quotedSelection: quotedSelections[0] ?? null,
    ...(s.currentSessionPath
      ? { quotedSelectionsBySession: putSessionBucket(s.quotedSelectionsBySession, s.currentSessionPath, quotedSelections, sessionScopedKey(s as never, s.currentSessionPath) || s.currentSessionPath) }
      : {}),
    ...bumpComposerRevision(s as never, currentComposerRevisionKey(s as never)),
  };
}

function clearPrimaryQuotedSelections(s: ComposerSessionState): Partial<InputSlice> {
  return {
    quotedSelections: [],
    quotedSelection: null,
    ...(s.currentSessionPath
      ? { quotedSelectionsBySession: deleteSessionBucket(s.quotedSelectionsBySession, s.currentSessionPath, sessionScopedKey(s as never, s.currentSessionPath) || s.currentSessionPath) }
      : {}),
  };
}

export const createInputSlice = (
  set: (partial: Partial<InputSlice> | ((s: InputSlice) => Partial<InputSlice>)) => void
): InputSlice => ({
  attachedFiles: [],
  attachedFilesBySession: {},
  drafts: {},
  draftDocs: {},
  composerRevisionsByKey: {},
  draftsHydratedAt: 0,
  deskContextAttached: false,
  docContextAttached: false,
  docContextAttachedBySession: {},
  inputFocusTrigger: 0,
  inputFocusTriggerSource: 'gesture',
  quoteCandidate: null,
  quotedSelections: [],
  quotedSelectionsBySession: {},
  quotedSelection: null,
  addAttachedFile: (file) =>
    set((s) => ({
      ...syncCurrentSessionAttachments(s as InputSlice & { currentSessionPath?: string | null }, [...s.attachedFiles, file]),
      ...bumpComposerRevision(s as never, currentComposerRevisionKey(s as never)),
    })),
  removeAttachedFile: (index) =>
    set((s) => ({
      ...syncCurrentSessionAttachments(
        s as InputSlice & { currentSessionPath?: string | null },
        s.attachedFiles.filter((_, i) => i !== index),
      ),
      ...bumpComposerRevision(s as never, currentComposerRevisionKey(s as never)),
    })),
  setAttachedFiles: (files) =>
    set((s) => ({
      ...syncCurrentSessionAttachments(s as InputSlice & { currentSessionPath?: string | null }, files),
      ...bumpComposerRevision(s as never, currentComposerRevisionKey(s as never)),
    })),
  clearAttachedFiles: () =>
    set((s) => syncCurrentSessionAttachments(s as InputSlice & { currentSessionPath?: string | null }, [])),
  clearAttachedFilesForSession: (sessionPath) =>
    set((s) => {
      const state = s as InputSlice & { currentSessionPath?: string | null };
      const key = sessionScopedKey(state as any, sessionPath) || sessionPath;
      const attachedFilesBySession = { ...s.attachedFilesBySession };
      delete attachedFilesBySession[key];
      delete attachedFilesBySession[sessionPath];
      return {
        attachedFilesBySession,
        ...(state.currentSessionPath === sessionPath ? { attachedFiles: [] } : {}),
      };
    }),
  setDraft: (sessionPath, text, doc) =>
    set((s) => {
      const key = sessionScopedKey(s as any, sessionPath) || sessionPath;
      const prevText = Object.prototype.hasOwnProperty.call(s.drafts, key)
        ? s.drafts[key]
        : Object.prototype.hasOwnProperty.call(s.drafts, sessionPath)
          ? s.drafts[sessionPath]
          : undefined;
      const prevDoc = Object.prototype.hasOwnProperty.call(s.draftDocs, key)
        ? s.draftDocs[key]
        : Object.prototype.hasOwnProperty.call(s.draftDocs, sessionPath)
          ? s.draftDocs[sessionPath]
          : undefined;
      const nextDoc = doc ?? null;
      const sameText = prevText === text;
      const sameDoc = nextDoc
        ? !!prevDoc && JSON.stringify(prevDoc) === JSON.stringify(nextDoc)
        : prevDoc === undefined;
      // 内容未变则保持 drafts/draftDocs 引用不变，避免 InputArea 订阅方空转，
      // 也让「恢复走 emitUpdate」时不会 update→setDraft→effect 死循环。
      if (sameText && sameDoc && prevText !== undefined) {
        return {};
      }
      const drafts = { ...s.drafts, [key]: text };
      const draftDocs = { ...s.draftDocs };
      if (doc) draftDocs[key] = doc;
      else delete draftDocs[key];
      if (key !== sessionPath) delete drafts[sessionPath];
      if (key !== sessionPath) delete draftDocs[sessionPath];
      notifyDraftSet(key, text, doc ?? null);
      // 内容确实变化才递增修订号；清空/恢复触发的幂等回写不算编辑。
      return { drafts, draftDocs, ...bumpComposerRevision(s as never, key) };
    }),
  clearDraft: (sessionPath) =>
    set((s) => {
      const key = sessionScopedKey(s as any, sessionPath) || sessionPath;
      const rest = { ...s.drafts };
      const draftDocs = { ...s.draftDocs };
      delete rest[key];
      delete rest[sessionPath];
      delete draftDocs[key];
      delete draftDocs[sessionPath];
      notifyDraftCleared(key);
      return { drafts: rest, draftDocs };
    }),
  setDeskContextAttached: (attached) => set({ deskContextAttached: attached }),
  toggleDeskContext: () =>
    set((s) => ({ deskContextAttached: !s.deskContextAttached })),
  setDocContextAttached: (attached) =>
    set((s) => {
      const state = s as ComposerSessionState;
      return {
        docContextAttached: attached,
        ...(state.currentSessionPath
          ? { docContextAttachedBySession: putSessionBucket(state.docContextAttachedBySession, state.currentSessionPath, attached, sessionScopedKey(state as never, state.currentSessionPath) || state.currentSessionPath) }
          : {}),
        ...bumpComposerRevision(state as never, currentComposerRevisionKey(state as never)),
      };
    }),
  setDocContextAttachedForSession: (sessionPath, attached) =>
    set((s) => {
      const key = sessionScopedKey(s as never, sessionPath) || sessionPath;
      return {
        docContextAttachedBySession: putSessionBucket(s.docContextAttachedBySession, sessionPath, attached, key),
        ...(isPrimarySessionTarget(s as never, sessionPath) ? { docContextAttached: attached } : {}),
        ...bumpComposerRevision(s as never, key),
      };
    }),
  toggleDocContext: () =>
    set((s) => {
      const state = s as ComposerSessionState;
      const attached = !state.docContextAttached;
      return {
        docContextAttached: attached,
        ...(state.currentSessionPath
          ? { docContextAttachedBySession: putSessionBucket(state.docContextAttachedBySession, state.currentSessionPath, attached, sessionScopedKey(state as never, state.currentSessionPath) || state.currentSessionPath) }
          : {}),
        ...bumpComposerRevision(state as never, currentComposerRevisionKey(state as never)),
      };
    }),
  requestInputFocus: (source = 'gesture') =>
    set((s) => ({ inputFocusTrigger: s.inputFocusTrigger + 1, inputFocusTriggerSource: source })),
  setQuoteCandidate: (sel) => set({ quoteCandidate: sel }),
  clearQuoteCandidate: () => set({ quoteCandidate: null }),
  addQuotedSelection: (sel) => set((s) => addPrimaryQuotedSelection(s as never, sel)),
  removeQuotedSelection: (index) => set((s) => removePrimaryQuotedSelection(s as never, index)),
  clearQuotedSelections: () => set((s) => clearPrimaryQuotedSelections(s as never)),
  setQuotedSelections: (sels) =>
    set((s) => {
      const state = s as ComposerSessionState;
      return {
        quotedSelections: sels,
        quotedSelection: sels[0] ?? null,
        ...(state.currentSessionPath
          ? { quotedSelectionsBySession: putSessionBucket(state.quotedSelectionsBySession, state.currentSessionPath, sels, sessionScopedKey(state as never, state.currentSessionPath) || state.currentSessionPath) }
          : {}),
        ...bumpComposerRevision(state as never, currentComposerRevisionKey(state as never)),
      };
    }),
  addQuotedSelectionForSession: (sessionPath, sel) =>
    set((s) => {
      if (isPrimarySessionTarget(s as never, sessionPath)) {
        return addPrimaryQuotedSelection(s, sel);
      }
      const key = sessionScopedKey(s as never, sessionPath) || sessionPath;
      const next = [...(sessionBucketValue(s as never, s.quotedSelectionsBySession, sessionPath) ?? []), sel];
      return {
        quotedSelectionsBySession: putSessionBucket(s.quotedSelectionsBySession, sessionPath, next, key),
        ...bumpComposerRevision(s as never, key),
      };
    }),
  removeQuotedSelectionForSession: (sessionPath, index) =>
    set((s) => {
      if (isPrimarySessionTarget(s as never, sessionPath)) {
        return removePrimaryQuotedSelection(s, index);
      }
      const key = sessionScopedKey(s as never, sessionPath) || sessionPath;
      const next = (sessionBucketValue(s as never, s.quotedSelectionsBySession, sessionPath) ?? []).filter((_, i) => i !== index);
      return {
        quotedSelectionsBySession: putSessionBucket(s.quotedSelectionsBySession, sessionPath, next, key),
        ...bumpComposerRevision(s as never, key),
      };
    }),
  clearQuotedSelectionsForSession: (sessionPath) =>
    set((s) => {
      if (isPrimarySessionTarget(s as never, sessionPath)) {
        return clearPrimaryQuotedSelections(s);
      }
      const key = sessionScopedKey(s as never, sessionPath) || sessionPath;
      return {
        quotedSelectionsBySession: deleteSessionBucket(s.quotedSelectionsBySession, sessionPath, key),
      };
    }),
  setQuotedSelection: (sel) => set((s) => addPrimaryQuotedSelection(
    { ...(s as ComposerSessionState & InputSlice), quotedSelections: [] },
    sel,
  )),
  clearQuotedSelection: () => set((s) => ({
    ...clearPrimaryQuotedSelections(s as never),
    quoteCandidate: null,
  })),
});
