/**
 * @vitest-environment jsdom
 *
 * Phase 10.1：provenance 损坏必须在载荷卡片中显示为损坏，不能伪装成缺失。
 */
import React from 'react';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import type {
  ModelObservabilityPayloadRecordDetail,
  ModelObservabilityPayloadRecordMetadata,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { ObservabilityPayloadCard } from '../../../settings/tabs/observability/ObservabilityPayloadCard';

const metadata: ModelObservabilityPayloadRecordMetadata = {
  id: 1,
  callId: 'mc_truth',
  kind: 'semantic_request',
  attemptId: null,
  providerRequestOrdinal: null,
  capturedAt: '2026-08-22T00:00:00.000Z',
  visibility: 'full',
  fidelity: 'runtime_exact',
  sanitizationStatus: 'none',
  redacted: false,
  truncated: false,
  degraded: false,
  recordCharCount: 10,
  hasBody: true,
  hasSemanticProvenance: true,
  hasProviderProvenance: false,
  blobIds: [],
};

describe('ObservabilityPayloadCard provenance 真值', () => {
  beforeAll(() => {
    window.t = ((key: string) => key) as typeof window.t;
  });

  afterEach(() => cleanup());

  it('semantic provenance JSON 损坏显示 corrupt，不显示 absent', () => {
    const detail: ModelObservabilityPayloadRecordDetail = {
      ...metadata,
      contentAvailable: true,
      contentState: 'present',
      payload: { prompt: 'safe' },
      semanticInputProvenanceState: 'corrupt',
      semanticInputProvenance: null,
      providerRequestProvenanceState: 'absent',
      providerRequestProvenance: null,
    };
    render(<ObservabilityPayloadCard
      metadata={metadata}
      body={{ status: 'loaded', detail }}
      isLocalOwner
      onLoadBody={() => {}}
    />);
    expect(screen.getByText('settings.observability.provenance.corrupt')).toBeInTheDocument();
    expect(screen.queryByText('settings.observability.provenance.absent')).toBeNull();
  });
});
