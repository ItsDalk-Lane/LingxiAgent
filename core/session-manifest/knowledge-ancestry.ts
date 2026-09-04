import path from "node:path";
import { KnowledgeError, isKnowledgeError } from "../../lib/knowledge/errors.ts";
import { knowledgeScopeViolation, sameKnowledgeSessionPath, type KnowledgeToolSessionContext } from "../../lib/tools/knowledge-scope.ts";
import type { SessionManifestStore } from "./store.ts";

type KnowledgeSessionManifest = NonNullable<ReturnType<SessionManifestStore["getBySessionId"]>>;

export interface KnowledgeScopeSessionResolverInput {
  sessionPath: string | null;
  studioId: string;
  getSessionIdForPath: (sessionPath: string) => string | null;
  /** 回调必须绑定当前 Studio 的登记库，不能读取模型提交的会话身份。 */
  getSessionManifest: (sessionId: string) => KnowledgeSessionManifest | null;
}

function nonEmpty(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

/** 沿真实会话登记向上找知识范围拥有者，最多追溯八层父会话。 */
export function resolveKnowledgeScopeSessionContext(input: KnowledgeScopeSessionResolverInput): KnowledgeToolSessionContext {
  if (!nonEmpty(input.sessionPath)) {
    throw new KnowledgeError("KNOWLEDGE_MODEL_UNAVAILABLE", "Knowledge tools require a session-bound context");
  }
  if (!path.isAbsolute(input.sessionPath) || !nonEmpty(input.studioId)) {
    throw knowledgeScopeViolation("Knowledge session identity cannot be resolved");
  }
  try {
    let sessionId = input.getSessionIdForPath(input.sessionPath);
    const visited = new Set<string>();
    for (let ancestors = 0; ancestors <= 8; ancestors++) {
      if (!nonEmpty(sessionId) || visited.has(sessionId)) {
        throw knowledgeScopeViolation("Knowledge session ancestry is missing or cyclic");
      }
      visited.add(sessionId);
      const manifest = input.getSessionManifest(sessionId);
      if (!manifest || manifest.sessionId !== sessionId || manifest.lifecycle !== "active"
        || !nonEmpty(manifest.currentLocator?.path) || !path.isAbsolute(manifest.currentLocator.path)
        || input.getSessionIdForPath(manifest.currentLocator.path) !== sessionId) {
        throw knowledgeScopeViolation("Knowledge session manifest or locator is unavailable");
      }
      if (ancestors === 0 && !sameKnowledgeSessionPath(input.sessionPath, manifest.currentLocator.path)) {
        throw knowledgeScopeViolation("Knowledge session locator does not match the active session");
      }
      const provenance = manifest.provenance;
      if (!provenance || typeof provenance !== "object" || Array.isArray(provenance)) {
        throw knowledgeScopeViolation("Knowledge session provenance is invalid");
      }
      // 普通历史会话没有 Studio 字段，隔离性由绑定当前 Studio 的登记库保证；显式声明不可相互矛盾。
      const declaredStudio = (manifest as KnowledgeSessionManifest & { studioId?: unknown }).studioId;
      if ((declaredStudio !== undefined && declaredStudio !== input.studioId)
        || (provenance.studioId !== undefined && provenance.studioId !== input.studioId)) {
        throw knowledgeScopeViolation("Knowledge session ancestry crosses studios");
      }
      if (!nonEmpty(manifest.kind) || !nonEmpty(manifest.domain)) {
        throw knowledgeScopeViolation("Knowledge session kind cannot be resolved");
      }
      const isChild = manifest.domain === "subagent" || manifest.kind === "subagent_child"
        || manifest.kind === "knowledge_research_root" || manifest.kind === "knowledge_research_worker"
        || manifest.kind === "knowledge_completeness_worker";
      if (!isChild) {
        return { sessionPath: input.sessionPath, scopeOwnerSessionPath: manifest.currentLocator.path };
      }
      if (ancestors === 8 || !nonEmpty(provenance.parentSessionId)) {
        throw knowledgeScopeViolation("Knowledge session ancestry exceeds its limit or has no parent");
      }
      sessionId = provenance.parentSessionId;
    }
  } catch (error) {
    if (isKnowledgeError(error)) throw error;
    throw knowledgeScopeViolation("Knowledge session ancestry cannot be resolved");
  }
  throw knowledgeScopeViolation("Knowledge session ancestry cannot be resolved");
}
