/**
 * observability-export-save.ts — 导出流式保存编排（Phase 9 §一百一十五～
 * 一百一十八）。
 *
 * 红线：禁止 res.text()/res.blob() 全量缓冲（导出可能数百 MB）——一律
 * reader.read() 分块 → save bridge。
 *
 * 双通道：
 *   1. Electron IPC 桥（observabilityExportBegin/Write/End/Abort）：用户先
 *      在系统对话框选路径；abort 删除部分文件。
 *   2. File System Access API（showSaveFilePicker，浏览器 dev fallback）：
 *      abort 无法删除已写部分（API 限制）——结果里如实标注 partialLeft。
 * 两者皆无 → capability null（导出按钮禁用，§一百一十六）。
 */
import type { ModelObservabilityExportRequest } from '../../../../../../shared/model-observability-api-contract.ts';
import { fetchObservabilityExportStream, isObservabilityAbortError } from './model-observability-actions';

/* ── File System Access API 的最小类型（TS lib 未含）─────────────────── */

type FsaWritable = {
  write: (chunk: Uint8Array) => Promise<void>;
  close: () => Promise<void>;
  abort: () => Promise<void>;
};
type FsaFileHandle = { createWritable: () => Promise<FsaWritable> };
type ShowSaveFilePicker = (options: { suggestedName?: string }) => Promise<FsaFileHandle>;

function showSaveFilePicker(): ShowSaveFilePicker | null {
  const candidate = (window as unknown as { showSaveFilePicker?: unknown }).showSaveFilePicker;
  return typeof candidate === 'function' ? candidate as ShowSaveFilePicker : null;
}

export type ObservabilityExportCapability = 'electron' | 'file-system-access';

/** 导出可用通道；null = 不可用（UI 禁用导出并给原因）。 */
export function observabilityExportCapability(): ObservabilityExportCapability | null {
  const platform = window.hana;
  if (platform?.observabilityExportBegin && platform.observabilityExportWrite
    && platform.observabilityExportEnd && platform.observabilityExportAbort) {
    return 'electron';
  }
  if (showSaveFilePicker()) return 'file-system-access';
  return null;
}

/** §一百一十七：文件名只含产品名 + 时间戳（绝无 session 名/prompt/agent 名）。 */
export function observabilityExportFileName(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
  return `lingxi-model-observability-${stamp}.jsonl`;
}

export type ObservabilityExportOutcome =
  | { outcome: 'saved'; filePath: string | null; bytesWritten: number }
  | { outcome: 'canceled' }
  | { outcome: 'aborted'; partialLeft: boolean };

export async function runObservabilityExport(opts: {
  request: ModelObservabilityExportRequest;
  capability: ObservabilityExportCapability;
  defaultFileName: string;
  signal?: AbortSignal;
  onProgress?: (bytesWritten: number) => void;
}): Promise<ObservabilityExportOutcome> {
  const { request, capability, defaultFileName, signal, onProgress } = opts;

  if (capability === 'electron') {
    const platform = window.hana!;
    const begin = await platform.observabilityExportBegin!({ defaultFileName });
    if (begin.canceled) return { outcome: 'canceled' };
    const exportId = begin.exportId;
    let bytesWritten = 0;
    try {
      const res = await fetchObservabilityExportStream(request, { signal });
      if (!res.body) throw new Error('export response has no body stream');
      const reader = res.body.getReader();
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        if (value && value.byteLength > 0) {
          const written = await platform.observabilityExportWrite!({ exportId, chunk: value });
          bytesWritten = written.bytesWritten;
          onProgress?.(bytesWritten);
        }
      }
      const ended = await platform.observabilityExportEnd!({ exportId });
      return { outcome: 'saved', filePath: ended.filePath, bytesWritten: ended.bytesWritten };
    } catch (error) {
      await platform.observabilityExportAbort!({ exportId }).catch(() => {});
      if (isObservabilityAbortError(error)) return { outcome: 'aborted', partialLeft: false };
      throw error;
    }
  }

  // file-system-access 通道
  const picker = showSaveFilePicker();
  if (!picker) throw new Error('no export capability');
  let handle: FsaFileHandle;
  try {
    handle = await picker({ suggestedName: defaultFileName });
  } catch (error) {
    if (isObservabilityAbortError(error)) return { outcome: 'canceled' };
    throw error;
  }
  const writable = await handle.createWritable();
  let bytesWritten = 0;
  try {
    const res = await fetchObservabilityExportStream(request, { signal });
    if (!res.body) throw new Error('export response has no body stream');
    const reader = res.body.getReader();
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      if (value && value.byteLength > 0) {
        await writable.write(value);
        bytesWritten += value.byteLength;
        onProgress?.(bytesWritten);
      }
    }
    await writable.close();
    return { outcome: 'saved', filePath: null, bytesWritten };
  } catch (error) {
    await writable.abort().catch(() => {});
    // FSA 限制：abort 不能删除已写部分——如实标注 partialLeft。
    if (isObservabilityAbortError(error)) return { outcome: 'aborted', partialLeft: bytesWritten > 0 };
    throw error;
  }
}
