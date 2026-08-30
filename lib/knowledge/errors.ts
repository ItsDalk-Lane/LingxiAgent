export type KnowledgeErrorCode =
  | "KNOWLEDGE_INVALID_ARGUMENT"
  | "KNOWLEDGE_NOT_FOUND"
  | "KNOWLEDGE_CONFLICT"
  | "KNOWLEDGE_SCHEMA_NEWER"
  | "KNOWLEDGE_STORAGE_INVALID"
  | "KNOWLEDGE_IMPORT_PATH_INVALID"
  | "KNOWLEDGE_IMPORT_NOT_FOUND"
  | "KNOWLEDGE_IMPORT_SYMLINK"
  | "KNOWLEDGE_IMPORT_FILE_REQUIRED"
  | "KNOWLEDGE_IMPORT_PATH_BLOCKED"
  | "KNOWLEDGE_IMPORT_TOO_LARGE"
  | "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED"
  | "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE"
  | "KNOWLEDGE_PARSE_FAILED"
  | "KNOWLEDGE_PARSE_NOT_READY"
  | "KNOWLEDGE_SCOPE_VIOLATION"
  | "KNOWLEDGE_SCOPE_EMPTY"
  | "KNOWLEDGE_SCOPE_NOT_READY"
  | "KNOWLEDGE_INDEX_INVALID"
  | "KNOWLEDGE_RETRIEVAL_EMPTY"
  | "KNOWLEDGE_RETRIEVAL_UNAVAILABLE"
  | "KNOWLEDGE_MODEL_UNAVAILABLE"
  | "KNOWLEDGE_MODEL_OUTPUT_INVALID"
  | "KNOWLEDGE_WEB_URL_BLOCKED"
  | "KNOWLEDGE_WEB_FETCH_FAILED"
  | "KNOWLEDGE_WEB_TOO_LARGE"
  | "KNOWLEDGE_WEB_TYPE_UNSUPPORTED";

/**
 * Knowledge 对外只暴露稳定错误码和脱敏消息。
 * 原始路径、正文和数据库错误不得通过这里泄露给接口调用方。
 */
export class KnowledgeError extends Error {
  declare code: KnowledgeErrorCode;
  declare details: Record<string, unknown>;

  constructor(code: KnowledgeErrorCode, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.name = "KnowledgeError";
    this.code = code;
    this.details = details;
  }
}

export function isKnowledgeError(error: unknown): error is KnowledgeError {
  return error instanceof KnowledgeError;
}
