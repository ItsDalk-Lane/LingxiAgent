import {
  MediaExecutionTargetError,
  type MediaCredentialSource,
  type MediaExecutionModality,
  type MediaExecutionTarget,
} from "./media-execution-target.ts";

type CredentialLane = {
  id?: unknown;
  providerId?: unknown;
  authType?: unknown;
  credentialSource?: unknown;
};

type ProviderCredentials = {
  apiKey?: unknown;
  headers?: unknown;
} | null;

export interface MediaExecutionProviderRegistry {
  get(providerId: string): Record<string, unknown> | null | undefined;
  getAuthType?(providerId: string): unknown;
  getCredentials(providerId: string): ProviderCredentials;
  getMediaProviderCredentialStatus?(
    providerId: string,
    capability: string,
  ): {
    activeProviderId?: unknown;
    activeLaneId?: unknown;
    lanes?: unknown;
    unavailableReason?: unknown;
  } | null | undefined;
}

export interface ResolveMediaExecutionTargetInput {
  modelId: string;
  modality: MediaExecutionModality;
  runtimeProviderId: string;
  credentialLane?: CredentialLane | null;
  adapterId?: string | null;
  providerRegistry: MediaExecutionProviderRegistry;
}

type CredentialState = {
  known: boolean;
  usable: boolean;
  authType: string;
  source: MediaCredentialSource;
};

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function capabilityFor(modality: MediaExecutionModality): string {
  if (modality === "video") return "video_generation";
  if (modality === "speech-recognition") return "speech_recognition";
  return "image_generation";
}

function credentialSource(
  authType: string,
  declaredSource: unknown,
): MediaCredentialSource {
  if (declaredSource === "auth-storage" || declaredSource === "external") {
    return declaredSource;
  }
  if (authType === "none") return "none";
  if (authType === "oauth") return "auth-storage";
  return "provider-registry";
}

function hasCredentialMaterial(credentials: ProviderCredentials): boolean {
  if (!credentials || typeof credentials !== "object") return false;
  if (text(credentials.apiKey)) return true;
  if (!credentials.headers || typeof credentials.headers !== "object") return false;
  return Object.keys(credentials.headers).length > 0;
}

function inspectCredentialProvider(
  registry: MediaExecutionProviderRegistry,
  providerId: string,
  lane?: CredentialLane | null,
): CredentialState {
  const entry = registry.get(providerId);
  if (!entry) {
    return {
      known: false,
      usable: false,
      authType: "unknown",
      source: "provider-registry",
    };
  }
  const authType = text(lane?.authType)
    || text(registry.getAuthType?.(providerId))
    || text(entry.authType)
    || "api-key";
  return {
    known: true,
    usable: authType === "none" || hasCredentialMaterial(registry.getCredentials(providerId)),
    authType,
    source: credentialSource(authType, lane?.credentialSource),
  };
}

function target(
  input: ResolveMediaExecutionTargetInput,
  {
    credentialProviderId,
    credentialLaneId,
    credentialSource: source,
    resolutionReason,
  }: Pick<
    MediaExecutionTarget,
    "credentialProviderId" | "credentialLaneId" | "credentialSource" | "resolutionReason"
  >,
): MediaExecutionTarget {
  return Object.freeze({
    modelId: input.modelId.trim(),
    modality: input.modality,
    runtimeProviderId: input.runtimeProviderId.trim(),
    credentialProviderId,
    credentialLaneId,
    credentialSource: source,
    adapterId: text(input.adapterId),
    resolutionReason,
  });
}

export function resolveMediaExecutionTarget(
  input: ResolveMediaExecutionTargetInput,
): MediaExecutionTarget {
  const modelId = text(input?.modelId);
  const runtimeProviderId = text(input?.runtimeProviderId);
  const registry = input?.providerRegistry;
  if (!modelId || !runtimeProviderId || !registry) {
    throw new MediaExecutionTargetError({
      code: "CREDENTIAL_PROVIDER_UNRESOLVED",
      message: "Media execution target is missing a model or runtime provider.",
      details: { resolutionReason: "execution_target_incomplete" },
    });
  }

  const explicitProviderId = text(input.credentialLane?.providerId);
  if (explicitProviderId) {
    const explicitState = inspectCredentialProvider(
      registry,
      explicitProviderId,
      input.credentialLane,
    );
    if (explicitState.usable) {
      return target(input, {
        credentialProviderId: explicitProviderId,
        credentialLaneId: text(input.credentialLane?.id),
        credentialSource: explicitState.source,
        resolutionReason: "explicit_credential_lane",
      });
    }
  }

  const credentialStatus = registry.getMediaProviderCredentialStatus?.(
    runtimeProviderId,
    capabilityFor(input.modality),
  ) || {};
  const activeProviderId = text(credentialStatus.activeProviderId);
  const lanes = Array.isArray(credentialStatus.lanes)
    ? credentialStatus.lanes.filter((lane): lane is CredentialLane => (
      !!lane && typeof lane === "object" && !Array.isArray(lane)
    ))
    : [];
  const activeLaneId = text(credentialStatus.activeLaneId);
  const activeLane = lanes.find((lane) => text(lane.id) === activeLaneId)
    || lanes.find((lane) => text(lane.providerId) === activeProviderId)
    || null;
  if (activeProviderId) {
    const activeState = inspectCredentialProvider(registry, activeProviderId, activeLane);
    if (activeState.usable) {
      return target(input, {
        credentialProviderId: activeProviderId,
        credentialLaneId: activeLaneId || text(activeLane?.id),
        credentialSource: activeState.source,
        resolutionReason: "active_provider_registry_lane",
      });
    }
  }

  const runtimeLane = lanes.find((lane) => text(lane.providerId) === runtimeProviderId) || null;
  const runtimeState = inspectCredentialProvider(registry, runtimeProviderId, runtimeLane);
  if (runtimeState.usable) {
    return target(input, {
      credentialProviderId: runtimeProviderId,
      credentialLaneId: text(runtimeLane?.id),
      credentialSource: runtimeState.source,
      resolutionReason: runtimeState.authType === "none"
        ? "runtime_provider_auth_none"
        : "runtime_provider_credentials",
    });
  }

  const code = runtimeState.known
    ? "CREDENTIAL_MISSING"
    : "CREDENTIAL_PROVIDER_UNRESOLVED";
  throw new MediaExecutionTargetError({
    code,
    message: code === "CREDENTIAL_MISSING"
      ? "No valid credentials are available for the media execution target."
      : "No credential provider can be resolved for the media execution target.",
    details: {
      modelId,
      modality: input.modality,
      runtimeProviderId,
      resolutionReason: code === "CREDENTIAL_MISSING"
        ? text(credentialStatus.unavailableReason) || "no_valid_credentials"
        : "runtime_provider_not_registered",
    },
  });
}

export { MediaExecutionTargetError } from "./media-execution-target.ts";
