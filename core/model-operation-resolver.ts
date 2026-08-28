import { composeResolvedModelExecution } from "./model-execution-config.ts";
import {
  isModelOperation,
  modelSupportsOperation,
  type ModelOperation,
} from "../shared/model-operations.ts";

export class ModelOperationConfigurationError extends Error {
  readonly code: string;
  readonly operation: ModelOperation;
  readonly statusCode = 422;

  constructor(operation: ModelOperation, code: string, message: string) {
    super(message);
    this.name = "ModelOperationConfigurationError";
    this.operation = operation;
    this.code = code;
  }
}

export interface ModelOperationResolveDeps {
  getOperationModelRef: (operation: ModelOperation) => any | null;
  resolveOperationModel: (operation: ModelOperation, ref: any) => any | null;
  resolveProviderCredentialsFresh: (provider: string) => Promise<any>;
  getProviderCredentials: (provider: string) => any | null;
  allowsMissingApiKey?: (provider: string, baseUrl: string) => boolean;
}

function hasCredentialHeaders(credential: any): boolean {
  return !!credential?.headers
    && typeof credential.headers === "object"
    && Object.keys(credential.headers).length > 0;
}

function isLocalBaseUrl(baseUrl: string): boolean {
  if (!baseUrl) return false;
  try {
    const hostname = new URL(baseUrl).hostname;
    return hostname === "localhost" || hostname === "127.0.0.1" || hostname === "0.0.0.0";
  } catch {
    return false;
  }
}

export class ModelOperationResolver {
  private readonly deps: ModelOperationResolveDeps;

  constructor(deps: ModelOperationResolveDeps) {
    this.deps = deps;
  }

  private select(operation: ModelOperation) {
    if (!isModelOperation(operation)) {
      throw new Error(`unknown model operation "${operation}"`);
    }
    const ref = this.deps.getOperationModelRef(operation);
    if (!ref) return null;
    const model = this.deps.resolveOperationModel(operation, ref);
    if (!model || !modelSupportsOperation(model, operation)) {
      const refLabel = `${ref?.provider || "?"}/${ref?.id || "?"}`;
      throw new ModelOperationConfigurationError(
        operation,
        "model_not_found",
        `Configured ${operation} model was not found: ${refLabel}`,
      );
    }
    if (!model.operationProtocol) {
      throw new ModelOperationConfigurationError(
        operation,
        "protocol_missing",
        `Configured ${operation} model has no operation protocol`,
      );
    }
    return model;
  }

  private compose(operation: ModelOperation, model: any, credential: any) {
    const baseUrl = credential?.baseUrl || credential?.base_url || model?.baseUrl || "";
    const allowsMissingApiKey = this.deps.allowsMissingApiKey?.(model.provider, baseUrl)
      ?? isLocalBaseUrl(baseUrl);
    if (
      !baseUrl
      || (!credential?.apiKey && !credential?.api_key && !hasCredentialHeaders(credential) && !allowsMissingApiKey)
    ) {
      throw new ModelOperationConfigurationError(
        operation,
        "provider_missing_creds",
        `Provider credentials are unavailable for ${model.provider}`,
      );
    }
    const executionModel = {
      ...model,
      api: model.operationProtocol,
    };
    const resolved = composeResolvedModelExecution({ model: executionModel, credential });
    return {
      ...resolved,
      operation,
      api: model.operationProtocol,
    };
  }

  resolveSync(operation: ModelOperation) {
    const model = this.select(operation);
    if (!model) return null;
    return this.compose(operation, model, this.deps.getProviderCredentials(model.provider));
  }

  async resolveFresh(operation: ModelOperation) {
    const model = this.select(operation);
    if (!model) return null;
    const credential = await this.deps.resolveProviderCredentialsFresh(model.provider);
    return this.compose(operation, model, credential);
  }
}
