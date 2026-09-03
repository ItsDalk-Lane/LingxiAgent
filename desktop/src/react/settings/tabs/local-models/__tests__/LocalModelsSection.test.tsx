/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

const mocks = vi.hoisted(() => ({ lingxiFetch: vi.fn() }));

vi.mock('../../../api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
}));

vi.mock('../../../helpers', () => ({
  t: (key: string, params?: Record<string, unknown>) => params
    ? `${key} ${Object.values(params).join(' ')}`
    : key,
}));

import { LocalModelsSection } from '../LocalModelsSection';

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as Response;
}

const entries = [
  ['qwen3-embedding-0.6b', 'embedding', 'Qwen3 Embedding 0.6B', 'llama.cpp'],
  ['paddleocr-vl-0.9b', 'ocr', 'PaddleOCR-VL 0.9B', 'vl-sidecar'],
  ['glm-ocr', 'ocr', 'GLM-OCR', 'vl-sidecar'],
  ['sensevoice-small', 'stt', 'SenseVoice Small', 'sherpa-onnx'],
  ['qwen3-asr-1.7b', 'stt', 'Qwen3 ASR 1.7B', 'qwen3-asr-sidecar'],
  ['kokoro-82m', 'tts', 'Kokoro 82M', 'sherpa-onnx'],
  ['indextts-2.5', 'tts', 'IndexTTS 2.5', 'indextts-sidecar'],
  ['cosyvoice2-0.5b', 'tts', 'CosyVoice2 0.5B', 'cosyvoice-sidecar'],
] as const;

function localState() {
  return {
    config: {
      backend: 'auto',
      threads: 'auto',
      embedding: { batchSize: 32 },
      ocr: { defaultModel: '', maxPages: 25, maxPixelsPerPage: 16_000_000 },
      stt: { vadEnabled: true, chunkMs: 30_000 },
      tts: { streaming: true, defaultModel: '', voice: '', bridgeReply: false },
      useMmap: true,
      mlock: false,
      quantPreference: 'smallest',
      idleUnloadMs: { small: 300_000, large: 120_000 },
      memoryBudgetSmallMb: 1536,
      preloadSmall: false,
      download: { concurrency: 4, mirrorBaseUrl: '' },
    },
    manifest: { configured: true, warning: null, version: '2026.09.02' },
    catalog: entries.map(([id, category, displayName, runtimeId], index) => ({
      id, category, displayName, runtimeId, license: 'Apache-2.0',
      distributionStatus: index === 0 ? 'catalog-only' : 'manifest-available',
      variants: [{ quant: index === 5 ? 'fp32' : 'q4', tier: index === 5 ? 'small' : 'large', estimatedPeakRssMb: 600, default: true }],
    })),
    installed: [{ id: 'kokoro-82m', category: 'tts', quant: 'fp32', version: '2026.09.02', tier: 'small', bytes: 1_073_741_824, integrity: 'verified', licenseAvailable: true }],
    downloads: [{
      taskId: 'a'.repeat(64), assetId: 'model-sensevoice-small@q4', status: 'downloading',
      downloadedBytes: 16 * 1024 * 1024, totalBytes: 64 * 1024 * 1024,
      bytesPerSecond: 2 * 1024 * 1024, remainingMs: 24_000, error: null,
    }],
    instances: [],
    rejected: [{ name: 'broken@q4', reason: 'model.json is invalid' }],
    resources: {
      memoryBudgetSmallMb: 1536,
      reservations: [{ key: 'local:kokoro-82m@fp32@2026.09.02', tier: 'small', reservedMb: 512 }],
      largeSlot: { activeKey: null, queue: [] },
    },
  };
}

describe('LocalModelsSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(window, 'platform', {
      configurable: true,
      value: {
        selectFolder: vi.fn(async () => '/tmp/local-model'),
        openLocalModelsFolder: vi.fn(),
        getFilePath: vi.fn(() => '/tmp/dropped-model'),
      },
    });
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    mocks.lingxiFetch.mockImplementation((path: string) => Promise.resolve(jsonResponse(
      path === '/api/local-models' ? localState()
        : path === '/api/local-models/import/inspect' ? { hasModelJson: true, candidates: [] }
          : path.endsWith('/license') ? { content: 'Apache License fixture' }
          : { ok: true },
    )));
  });

  afterEach(() => cleanup());

  it('按四类展示稳定目录，并诚实禁用没有资产清单的下载', async () => {
    render(<LocalModelsSection />);
    for (const [, , name] of entries) expect(await screen.findByText(name)).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'settings.localModels.download' })[0]).toBeDisabled();
    expect(screen.getByText('2.0 MB/s')).toBeInTheDocument();
    expect(screen.getByText(/settings\.localModels\.remaining 24s/)).toBeInTheDocument();
  });

  it('可暂停和取消在途下载，且长安装请求不阻塞这些控制', async () => {
    render(<LocalModelsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.localModels.pause' }));
    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      `/api/local-models/downloads/${'a'.repeat(64)}/pause`, expect.objectContaining({ method: 'POST' }),
    ));
    fireEvent.click(screen.getByRole('button', { name: 'settings.localModels.cancel' }));
    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      `/api/local-models/downloads/${'a'.repeat(64)}`, expect.objectContaining({ method: 'DELETE' }),
    ));
  });

  it('保存后端配置，并在确认后删除已安装模型', async () => {
    render(<LocalModelsSection />);
    fireEvent.change(await screen.findByLabelText('settings.localModels.backend'), { target: { value: 'cpu' } });
    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      '/api/local-models/config', expect.objectContaining({ method: 'PUT' }),
    ));

    fireEvent.click(screen.getByRole('button', { name: 'settings.localModels.delete' }));
    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      '/api/local-models/models/tts/kokoro-82m/fp32', expect.objectContaining({ method: 'DELETE' }),
    ));
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining('1.0 GB'));
  });

  it('展示资源与隔离状态，并支持固定目录打开和拖放导入', async () => {
    render(<LocalModelsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.localModels.openDirectory' }));
    expect(window.platform.openLocalModelsFolder).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/settings\.localModels\.memoryStatus 512 1536/)).toBeInTheDocument();
    expect(screen.getByText(/settings\.localModels\.largeSlotIdle 0/)).toBeInTheDocument();
    expect(screen.getByText(/settings\.localModels\.rejectedImport broken@q4 model\.json is invalid/)).toBeInTheDocument();

    const file = new File(['model'], 'model-dir');
    fireEvent.drop(screen.getByText('settings.localModels.dropImport'), { dataTransfer: { files: [file] } });
    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      '/api/local-models/import', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ directory: '/tmp/dropped-model' }),
      }),
    ));
  });

  it('没有 model.json 时要求确认候选，并禁用缺少运行时的选项', async () => {
    mocks.lingxiFetch.mockImplementation((path: string) => Promise.resolve(jsonResponse(
      path === '/api/local-models' ? localState()
        : path === '/api/local-models/import/inspect' ? {
          hasModelJson: false,
          candidates: [
            { id: 'sensevoice-small', category: 'stt', displayName: 'SenseVoice Small', quant: 'int8', tier: 'small', runtimeReady: true },
            { id: 'kokoro-82m', category: 'tts', displayName: 'Kokoro 82M', quant: 'fp32', tier: 'small', runtimeReady: false },
          ],
        } : { ok: true },
    )));
    render(<LocalModelsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.localModels.import' }));
    expect(await screen.findByRole('dialog', { name: 'settings.localModels.manualImportTitle' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /Kokoro 82M/ })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'settings.localModels.confirmImport' }));
    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      '/api/local-models/import', expect.objectContaining({
        body: JSON.stringify({ directory: '/tmp/local-model', modelId: 'sensevoice-small', quant: 'int8' }),
      }),
    ));
  });

  it('读取并展示已声明的模型许可证', async () => {
    render(<LocalModelsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.localModels.viewLicense' }));
    expect(await screen.findByText('Apache License fixture')).toBeInTheDocument();
    expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/local-models/models/tts/kokoro-82m/fp32/license');
    fireEvent.click(screen.getByRole('button', { name: 'settings.localModels.closeLicense' }));
    expect(screen.queryByText('Apache License fixture')).not.toBeInTheDocument();
  });
});
