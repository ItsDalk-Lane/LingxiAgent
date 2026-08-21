import { normalizeProviderHeaders } from "../shared/provider-auth.ts";

export type TemporaryProviderCredentialSource = "request-draft" | "fresh-provider";
export type TemporaryProviderCredentialOperation = "catalog-read" | "connectivity-probe";

type BoundaryOptions = {
  providerId: string;
  source: TemporaryProviderCredentialSource;
  operation: TemporaryProviderCredentialOperation;
  apiKey?: string;
  headers?: Record<string, string>;
  audit?: ((event: Record<string, unknown>) => unknown) | null;
};

type ConsumeRequest = {
  providerId: string;
  operation: TemporaryProviderCredentialOperation;
};

class TemporaryProviderCredentialBoundary {
  #apiKey: string;
  #audit: ((event: Record<string, unknown>) => unknown) | null;
  #consumed = false;
  #headers: Record<string, string>;
  #operation: TemporaryProviderCredentialOperation;
  #providerId: string;
  #source: TemporaryProviderCredentialSource;

  constructor(options: BoundaryOptions) {
    this.#providerId = String(options.providerId || "").trim();
    this.#source = options.source;
    this.#operation = options.operation;
    this.#apiKey = typeof options.apiKey === "string" ? options.apiKey : "";
    this.#headers = normalizeProviderHeaders(options.headers || {});
    this.#audit = typeof options.audit === "function" ? options.audit : null;
    if (!this.#providerId) throw new Error("Temporary provider credential boundary requires providerId");
    if (!["request-draft", "fresh-provider"].includes(this.#source)) {
      throw new Error("Temporary provider credential boundary requires a valid source");
    }
    if (!["catalog-read", "connectivity-probe"].includes(this.#operation)) {
      throw new Error("Temporary provider credential boundary requires a valid operation");
    }
  }

  consume(request: ConsumeRequest) {
    if (this.#consumed) {
      throw new Error("Temporary provider credentials were already consumed");
    }
    this.#consumed = true;
    const allowed = request?.providerId === this.#providerId && request?.operation === this.#operation;
    this.#recordAudit(allowed ? "allowed" : "denied", allowed ? null : "scope_mismatch");
    if (!allowed) {
      throw new Error("Temporary provider credential scope mismatch");
    }
    return {
      apiKey: this.#apiKey,
      headers: { ...this.#headers },
    };
  }

  #recordAudit(result: "allowed" | "denied", errorCode: string | null) {
    try {
      this.#audit?.({
        action: "provider.temporary_credentials.consume",
        target: { kind: "provider", id: this.#providerId },
        result,
        errorCode,
        secretFields: ["apiKey", "headers"],
        metadata: {
          providerId: this.#providerId,
          source: this.#source,
          operation: this.#operation,
        },
      });
    } catch {
      // 审计写入失败不能把秘密换一条旁路送出去；网络请求仍按边界决定继续或拒绝。
    }
  }
}

export function createTemporaryProviderCredentialBoundary(options: BoundaryOptions) {
  return new TemporaryProviderCredentialBoundary(options);
}
