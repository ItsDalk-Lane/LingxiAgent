import type { ProviderMediaCapabilityBinding, MediaCapabilityKind } from '../../store';
import type {
  MediaProvider,
  SpeechProvider,
  MediaConfig,
  SpeechConfig,
  UseMediaSettingsDataResult,
} from '../../hooks/useMediaSettingsData';

export interface ResolvedMediaCapability {
  capability: MediaCapabilityKind;
  runtimeProviderId: string;
  credentialLaneId?: string;
  provider: MediaProvider | SpeechProvider | null;
  config: MediaConfig | SpeechConfig | null;
  loading: boolean;
  available: boolean;
}

/**
 * 把 summary 的媒体能力绑定 + media endpoint 数据解析为可渲染的能力列表。
 *
 * 关键契约：summary 明确声明 capability 时，即使 media endpoint 暂时加载失败，
 * 能力仍要保留（available=false / loading），绝不因为一次网络故障把能力图标隐藏。
 */
export function resolveProviderMediaCapabilities(
  bindings: ProviderMediaCapabilityBinding[] | undefined,
  media: UseMediaSettingsDataResult,
): ResolvedMediaCapability[] {
  return (bindings || []).map(binding => {
    const capability = binding.capability;
    const runtimeProviderId = binding.runtime_provider_id;
    let provider: MediaProvider | SpeechProvider | null = null;
    let config: MediaConfig | SpeechConfig | null = null;
    let loading = false;

    if (capability === 'imageGeneration') {
      provider = (media.image.providers[runtimeProviderId] as MediaProvider) || null;
      config = media.image.config;
      loading = media.image.loading;
    } else if (capability === 'videoGeneration') {
      provider = (media.video.providers[runtimeProviderId] as MediaProvider) || null;
      config = media.video.config;
      loading = media.video.loading;
    } else if (capability === 'speechRecognition') {
      provider = (media.speech.providers[runtimeProviderId] as SpeechProvider) || null;
      config = media.speech.config;
      loading = media.speech.loading;
    }

    return {
      capability,
      runtimeProviderId,
      ...(binding.credential_lane_id ? { credentialLaneId: binding.credential_lane_id } : {}),
      provider,
      config,
      loading,
      available: provider != null,
    };
  });
}
