export type MediaExecutionModality = "image" | "video" | "speech-recognition";

export type MediaCredentialSource =
  | "provider-registry"
  | "auth-storage"
  | "external"
  | "none";

export interface MediaExecutionTarget {
  readonly modelId: string;
  readonly modality: MediaExecutionModality;
  readonly runtimeProviderId: string;
  readonly credentialProviderId: string;
  readonly credentialLaneId: string | null;
  readonly credentialSource: MediaCredentialSource;
  readonly adapterId: string | null;
  readonly resolutionReason:
    | "explicit_credential_lane"
    | "active_provider_registry_lane"
    | "runtime_provider_auth_none"
    | "runtime_provider_credentials";
}

export type MediaExecutionTargetErrorCode =
  | "CREDENTIAL_PROVIDER_UNRESOLVED"
  | "CREDENTIAL_MISSING";

export class MediaExecutionTargetError extends Error {
  readonly code: MediaExecutionTargetErrorCode;
  readonly details: Readonly<Record<string, unknown>>;

  constructor({
    code,
    message,
    details,
  }: {
    code: MediaExecutionTargetErrorCode;
    message: string;
    details: Record<string, unknown>;
  }) {
    super(message);
    this.name = "MediaExecutionTargetError";
    this.code = code;
    this.details = Object.freeze({ ...details });
  }
}
