/**
 * @vitest-environment jsdom
 *
 * Phase 9 导出保存编排测试 — 流式分块（§一百一十五 禁全量缓冲）、文件名
 * 纪律（§一百一十七）、abort 语义（electron 删部分文件；FSA 如实 partialLeft）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const streamMock = vi.fn<typeof import('../../../settings/tabs/observability/model-observability-actions').fetchObservabilityExportStream>();

vi.mock('../../../settings/tabs/observability/model-observability-actions', async (importOriginal) => {
  const original = await importOriginal<typeof import('../../../settings/tabs/observability/model-observability-actions')>();
  return {
    ...original,
    fetchObservabilityExportStream: (...args: Parameters<typeof streamMock>) => streamMock(...args),
  };
});

import {
  observabilityExportCapability,
  observabilityExportFileName,
  runObservabilityExport,
} from '../../../settings/tabs/observability/observability-export-save';

function chunkStream(chunks: string[]): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      controller.close();
    },
  });
}

function abortingStream(): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      controller.enqueue(encoder.encode('{"a":1}\n'));
      controller.error(Object.assign(new Error('aborted'), { name: 'AbortError' }));
    },
  });
}

type Bridge = {
  begin: ReturnType<typeof vi.fn>;
  write: ReturnType<typeof vi.fn>;
  end: ReturnType<typeof vi.fn>;
  abort: ReturnType<typeof vi.fn>;
};

function installElectronBridge(overrides: Partial<Record<keyof Bridge, ReturnType<typeof vi.fn>>> = {}): Bridge {
  let cumulativeBytes = 0;
  const bridge: Bridge = {
    begin: vi.fn(async () => ({ canceled: false, exportId: 'exp-1', filePath: '/tmp/out.jsonl' })),
    write: vi.fn(async ({ chunk }: { chunk: Uint8Array }) => {
      cumulativeBytes += chunk.byteLength;
      return { bytesWritten: cumulativeBytes };
    }),
    end: vi.fn(async () => ({ filePath: '/tmp/out.jsonl', bytesWritten: 42 })),
    abort: vi.fn(async () => {}),
    ...overrides,
  } as Bridge;
  (window as unknown as { hana: unknown }).hana = {
    observabilityExportBegin: bridge.begin,
    observabilityExportWrite: bridge.write,
    observabilityExportEnd: bridge.end,
    observabilityExportAbort: bridge.abort,
  };
  return bridge;
}

const REQUEST = { filter: {}, includePayloads: false, maxCalls: 1000 };

describe('observabilityExportFileName (§一百一十七)', () => {
  it('is product name + timestamp only — never session/agent/prompt names', () => {
    const name = observabilityExportFileName(new Date(2026, 7, 22, 8, 5));
    expect(name).toBe('lingxi-model-observability-20260822-0805.jsonl');
    expect(name).toMatch(/^lingxi-model-observability-\d{8}-\d{4}\.jsonl$/);
    expect(name).not.toContain('sess');
    expect(name).not.toContain('agent');
  });

  it('zero-pads month/day/hour/minute', () => {
    expect(observabilityExportFileName(new Date(2026, 0, 3, 4, 7)))
      .toBe('lingxi-model-observability-20260103-0407.jsonl');
  });
});

describe('observabilityExportCapability', () => {
  afterEach(() => {
    delete (window as unknown as { hana?: unknown }).hana;
    delete (window as unknown as { showSaveFilePicker?: unknown }).showSaveFilePicker;
  });

  it('prefers the electron bridge when all four IPC methods exist', () => {
    installElectronBridge();
    expect(observabilityExportCapability()).toBe('electron');
  });

  it('falls back to File System Access API, then null', () => {
    (window as unknown as { showSaveFilePicker: unknown }).showSaveFilePicker = () => Promise.resolve({});
    expect(observabilityExportCapability()).toBe('file-system-access');
    expect(observabilityExportCapability).toBeDefined();
    delete (window as unknown as { showSaveFilePicker?: unknown }).showSaveFilePicker;
    expect(observabilityExportCapability()).toBeNull();
  });

  it('partial electron bridge (missing abort) is not treated as capable', () => {
    (window as unknown as { hana: unknown }).hana = {
      observabilityExportBegin: vi.fn(),
      observabilityExportWrite: vi.fn(),
      observabilityExportEnd: vi.fn(),
    };
    expect(observabilityExportCapability()).toBeNull();
  });
});

describe('runObservabilityExport — electron channel', () => {
  beforeEach(() => { streamMock.mockReset(); });
  afterEach(() => { delete (window as unknown as { hana?: unknown }).hana; });

  it('streams chunk-by-chunk through the bridge and finishes with end (§一百一十五)', async () => {
    const bridge = installElectronBridge();
    streamMock.mockResolvedValue(new Response(chunkStream(['{"a":1}\n', '{"a":2}\n'])));

    const progress: number[] = [];
    const outcome = await runObservabilityExport({
      request: REQUEST, capability: 'electron', defaultFileName: 'x.jsonl',
      onProgress: (bytes) => progress.push(bytes),
    });

    expect(outcome).toEqual({ outcome: 'saved', filePath: '/tmp/out.jsonl', bytesWritten: 42 });
    expect(bridge.write).toHaveBeenCalledTimes(2);
    expect(progress).toEqual([8, 16]);
    expect(bridge.abort).not.toHaveBeenCalled();
    // 请求体原样传给流接口（含 includePayloads 开关）。
    expect(streamMock).toHaveBeenCalledWith(REQUEST, expect.objectContaining({ signal: undefined }));
  });

  it('user cancel at the save dialog resolves to canceled without any stream request', async () => {
    const bridge = installElectronBridge({
      begin: vi.fn(async () => ({ canceled: true, exportId: null as unknown as string })),
    });
    const outcome = await runObservabilityExport({
      request: REQUEST, capability: 'electron', defaultFileName: 'x.jsonl',
    });
    expect(outcome).toEqual({ outcome: 'canceled' });
    expect(streamMock).not.toHaveBeenCalled();
    expect(bridge.write).not.toHaveBeenCalled();
  });

  it('abort mid-stream calls the abort IPC which deletes the partial file', async () => {
    const bridge = installElectronBridge();
    streamMock.mockResolvedValue(new Response(abortingStream()));

    const outcome = await runObservabilityExport({
      request: REQUEST, capability: 'electron', defaultFileName: 'x.jsonl',
    });

    expect(outcome).toEqual({ outcome: 'aborted', partialLeft: false });
    expect(bridge.abort).toHaveBeenCalledWith({ exportId: 'exp-1' });
    expect(bridge.end).not.toHaveBeenCalled();
  });
});

describe('runObservabilityExport — file-system-access channel', () => {
  beforeEach(() => { streamMock.mockReset(); });
  afterEach(() => {
    delete (window as unknown as { showSaveFilePicker?: unknown }).showSaveFilePicker;
  });

  function installFsaPicker() {
    const writable = {
      write: vi.fn(async () => {}),
      close: vi.fn(async () => {}),
      abort: vi.fn(async () => {}),
    };
    (window as unknown as { showSaveFilePicker: unknown }).showSaveFilePicker = vi.fn(async () => ({
      createWritable: async () => writable,
    }));
    return writable;
  }

  it('streams into the writable and closes', async () => {
    const writable = installFsaPicker();
    streamMock.mockResolvedValue(new Response(chunkStream(['x', 'y'])));

    const outcome = await runObservabilityExport({
      request: REQUEST, capability: 'file-system-access', defaultFileName: 'x.jsonl',
    });

    expect(outcome).toEqual({ outcome: 'saved', filePath: null, bytesWritten: 2 });
    expect(writable.write).toHaveBeenCalledTimes(2);
    expect(writable.close).toHaveBeenCalled();
  });

  it('abort cannot delete the partial file — outcome honestly reports partialLeft (§一百一十五)', async () => {
    const writable = installFsaPicker();
    let writes = 0;
    writable.write.mockImplementation(async () => {
      writes += 1;
      if (writes > 1) throw Object.assign(new Error('aborted'), { name: 'AbortError' });
    });
    streamMock.mockResolvedValue(new Response(chunkStream(['{"a":1}\n', '{"a":2}\n'])));

    const outcome = await runObservabilityExport({
      request: REQUEST, capability: 'file-system-access', defaultFileName: 'x.jsonl',
    });

    expect(outcome).toEqual({ outcome: 'aborted', partialLeft: true });
    expect(writable.abort).toHaveBeenCalled();
    expect(writable.close).not.toHaveBeenCalled();
  });

  it('picker dismissal resolves to canceled', async () => {
    (window as unknown as { showSaveFilePicker: unknown }).showSaveFilePicker = vi.fn(() => Promise.reject(
      Object.assign(new Error('dismissed'), { name: 'AbortError' }),
    ));
    const outcome = await runObservabilityExport({
      request: REQUEST, capability: 'file-system-access', defaultFileName: 'x.jsonl',
    });
    expect(outcome).toEqual({ outcome: 'canceled' });
  });
});
