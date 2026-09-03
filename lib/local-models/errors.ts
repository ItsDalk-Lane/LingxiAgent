export type LocalModelErrorCode =
  | "LOCAL_MODEL_ABORTED"
  | "LOCAL_MODEL_NOT_INSTALLED"
  | "LOCAL_MODEL_RUNTIME_MISSING"
  | "LOCAL_MODEL_MANIFEST_INVALID"
  | "LOCAL_MODEL_DOWNLOAD_NETWORK"
  | "LOCAL_MODEL_DOWNLOAD_INTEGRITY"
  | "LOCAL_MODEL_DISK_SPACE"
  | "LOCAL_MODEL_ARCHIVE_UNSAFE"
  | "LOCAL_MODEL_INSTALL_INVALID"
  | "LOCAL_MODEL_ALREADY_INSTALLED"
  | "LOCAL_MODEL_MEMORY_INSUFFICIENT"
  | "LOCAL_MODEL_BACKEND_UNAVAILABLE"
  | "LOCAL_MODEL_SIDECAR_FAILED"
  | "LOCAL_MODEL_UNSUPPORTED";

export class LocalModelError extends Error {
  readonly code: LocalModelErrorCode;
  readonly details: Readonly<Record<string, unknown>>;

  constructor(code: LocalModelErrorCode, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.name = "LocalModelError";
    this.code = code;
    this.details = Object.freeze({ ...details });
  }
}

export function localModelAbortError(message = "local model operation was cancelled"): LocalModelError {
  return new LocalModelError("LOCAL_MODEL_ABORTED", message);
}

export function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) throw localModelAbortError();
}

export function isLocalModelAbort(error: unknown): boolean {
  return error instanceof LocalModelError && error.code === "LOCAL_MODEL_ABORTED";
}
