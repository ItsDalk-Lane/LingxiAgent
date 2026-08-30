/**
 * knowledge 工具共享的 KnowledgeTurnScope 校验链（Phase 11 从 knowledge-read-tool
 * 的 Phase 4 实现原位抽取，供 knowledge_read / knowledge_outline / knowledge_grep
 * 复用；语义不变，任务书 §二十~§二十二）：
 *
 * - scopeId 必填且服务端逐次复核：scope 存在、active、属于当前 studio 与当前会话
 *   （subagent 子会话经 manifest provenance 继承父会话 scope——scope 只能缩小）；
 * - sourceId / notebookId 必须在 scope 冻结集合内，不信任模型传入的任何 id；
 * - 读取锚定 scope 冻结的 snapshot/artifact（§四十三）。任何一项失败 →
 *   KNOWLEDGE_SCOPE_VIOLATION / 显式错误，不回落到旧的全 studio 扫描行为。
 */
import path from "node:path";

import { KnowledgeError } from "../knowledge/errors.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import type { KnowledgeBlock, KnowledgeTurnScope, KnowledgeTurnScopeSource } from "../knowledge/types.ts";

/** 工具执行会话的 scope 归属上下文（Pi SDK execute 第 5 参 ctx 的解析结果）。 */
export interface KnowledgeToolSessionContext {
  sessionPath: string | null;
  parentSessionPath: string | null;
}

export function knowledgeScopeViolation(message: string): KnowledgeError {
  return new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", message);
}

export function sameKnowledgeSessionPath(left: string, right: string): boolean {
  return path.resolve(left) === path.resolve(right);
}

/**
 * 无会话上下文的 surface（如独立 CLI 调用）：显式不可用，不静默放行。
 * 返回可用的会话路径。
 */
export function requireKnowledgeSessionContext(
  sessionContext: KnowledgeToolSessionContext,
): string {
  if (!sessionContext.sessionPath) {
    throw new KnowledgeError(
      "KNOWLEDGE_MODEL_UNAVAILABLE",
      "this knowledge tool requires a session-bound KnowledgeTurnScope context",
    );
  }
  return sessionContext.sessionPath;
}

/**
 * scope 归属校验（服务端复核，不信任模型传入的 scopeId）：
 * 存在、active、属于当前 studio、属于当前会话（或其 subagent 父会话）。
 * 通过返回完整 scope（含冻结源集合）。
 */
export function resolveKnowledgeTurnScope(input: {
  knowledge: KnowledgeManager;
  studioId: string;
  scopeId: string;
  sessionContext: KnowledgeToolSessionContext;
}): KnowledgeTurnScope {
  requireKnowledgeSessionContext(input.sessionContext);
  const scope = input.knowledge.getTurnScope({ scopeId: input.scopeId });
  if (!scope) throw knowledgeScopeViolation("Unknown knowledge turn scope");
  if (scope.studioId !== input.studioId) {
    throw knowledgeScopeViolation("Knowledge turn scope belongs to a different studio");
  }
  if (scope.status !== "active") {
    throw knowledgeScopeViolation("Knowledge turn scope is closed (superseded by a newer turn)");
  }
  const ownsScope = sameKnowledgeSessionPath(scope.sessionPath, input.sessionContext.sessionPath!)
    || (input.sessionContext.parentSessionPath != null
      && sameKnowledgeSessionPath(scope.sessionPath, input.sessionContext.parentSessionPath));
  if (!ownsScope) {
    throw knowledgeScopeViolation("Knowledge turn scope does not belong to this session");
  }
  return scope;
}

/** sourceId 必须在 scope 冻结集合内；返回冻结条目（含 snapshot/artifact 身份）。 */
export function requireKnowledgeScopeSource(
  scope: KnowledgeTurnScope,
  sourceId: string,
): KnowledgeTurnScopeSource {
  const frozen = scope.sources.find(source => source.sourceId === sourceId);
  if (!frozen) {
    throw knowledgeScopeViolation("Knowledge source is outside this turn's scope");
  }
  return frozen;
}

/**
 * notebookId 归属解析：给出时必须同时属于 scope 选中集合与该源的冻结引用集合；
 * 缺失时取冻结集合内第一个引用笔记本（限选中集合，不扫全 studio）。
 */
export function resolveKnowledgeOwningNotebookId(
  scope: KnowledgeTurnScope,
  frozen: KnowledgeTurnScopeSource,
  notebookId: string | null,
): string {
  if (notebookId) {
    if (!scope.notebookIds.includes(notebookId) || !frozen.notebookIds.includes(notebookId)) {
      throw knowledgeScopeViolation("Notebook is outside this turn's scope for this source");
    }
    return notebookId;
  }
  return frozen.notebookIds[0];
}

/**
 * block 的有效 headingPath（与 chunker.headingPathOf 同口径）：locator.headingPath
 * 可能是稀疏数组经 JSON 序列化后的含 null 形态，只保留有效标题。
 */
export function knowledgeBlockHeadingPath(block: KnowledgeBlock): string[] {
  const raw = (block?.locator as Record<string, unknown> | undefined)?.headingPath;
  if (!Array.isArray(raw)) return [];
  return raw.filter((entry): entry is string => typeof entry === "string" && entry.trim().length > 0);
}
