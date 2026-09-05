import fs from "node:fs";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { afterEach, describe, expect, it, vi } from "vitest";

const facadeUrl = new URL("../lib/knowledge/knowledge-context-injector.ts", import.meta.url);
const legacyUrl = new URL("./fixtures/knowledge-legacy/legacy-knowledge-context-injector.ts", import.meta.url);
afterEach(() => { vi.doUnmock("./fixtures/knowledge-legacy/legacy-knowledge-context-injector.ts"); vi.resetModules(); });

describe("知识上下文门面与旧详细编排的模块边界", () => {
  it("加载生产门面无需加载旧编排，门面不再暴露旧调查入口", async () => {
    vi.resetModules();
    vi.doMock("./fixtures/knowledge-legacy/legacy-knowledge-context-injector.ts", () => {
      throw new Error("生产门面不得加载旧调查编排");
    });
    const facade = await import("../lib/knowledge/knowledge-context-injector.ts");
    expect(facade.EvidencePacker).toBeTypeOf("function");
    expect("renderKnowledgeContextBlock" in facade).toBe(false);
    expect(facade.assembleKnowledgeEvidenceManifestEntries).toBeTypeOf("function");
    for (const name of ["buildKnowledgeContextInjection", "decomposeQuestion", "decomposeQuestionAdaptive", "expandQueries", "runGapAnalysis"]) {
      expect(name in facade).toBe(false);
    }
    expect("resolveKnowledgeInjectionBudgetTokens" in facade).toBe(false);
  });

  it("旧入口仅在明确导入旧模块时可用，公共渲染和清单仍是门面的真实实现", async () => {
    const facade = await import("../lib/knowledge/knowledge-context-injector.ts");
    const legacy = await import("./fixtures/knowledge-legacy/legacy-knowledge-context-injector.ts");
    for (const name of ["buildKnowledgeContextInjection", "decomposeQuestion", "decomposeQuestionAdaptive", "expandQueries", "runGapAnalysis"] as const) {
      expect(legacy[name]).toBeTypeOf("function");
    }
    const renderer = await import("./fixtures/knowledge-legacy/legacy-context-renderer.ts");
    expect(legacy.renderKnowledgeContextBlock).toBe(renderer.renderKnowledgeContextBlock);
    expect(renderer.resolveKnowledgeInjectionBudgetTokens({ contextWindow: 32000, maxOutput: 4096 })).toBe(27904);
    expect(legacy.assembleKnowledgeEvidenceManifestEntries).toBe(facade.assembleKnowledgeEvidenceManifestEntries);
    expect(legacy.knowledgeModeGuidance).toBe(renderer.knowledgeModeGuidance);
  });

  it("门面没有反向导入旧模块或执行旧滚动链路，两个模块没有通配再导出", () => {
    const facade = ts.createSourceFile(fileURLToPath(facadeUrl), fs.readFileSync(facadeUrl, "utf8"), ts.ScriptTarget.Latest, true);
    const legacy = ts.createSourceFile(fileURLToPath(legacyUrl), fs.readFileSync(legacyUrl, "utf8"), ts.ScriptTarget.Latest, true);
    const paths: string[] = [], calls: string[] = [];
    const visit = (node: ts.Node) => {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier
        && ts.isStringLiteral(node.moduleSpecifier)) paths.push(node.moduleSpecifier.text);
      if (ts.isCallExpression(node)) calls.push(node.expression.getText(facade));
      ts.forEachChild(node, visit);
    };
    visit(facade);
    expect(paths.some(name => name.includes("legacy/"))).toBe(false);
    expect(calls).not.toContain("runKnowledgeRollup");
    expect(calls).not.toContain("import");
    const engine = fs.readFileSync(new URL("../core/engine.ts", import.meta.url), "utf8");
    expect(engine).not.toContain("buildKnowledgeContextInjection");
    expect(engine).not.toContain("runKnowledgeRollup");
    const manifest = fs.readFileSync(new URL("../export-manifest.json", import.meta.url), "utf8");
    expect(manifest).not.toContain("knowledge-legacy/");
    expect(manifest).not.toContain("legacy-knowledge-context-injector.ts");
    expect(manifest).not.toContain("knowledge-rollup.ts");
    const query = fs.readFileSync(new URL("../lib/knowledge/knowledge-query-service.ts", import.meta.url), "utf8");
    for (const retired of ["async retrieveForNotebooks(", "async retrieveForArtifacts(", "resolveRetrievalScopes(",
      "marginGate", "KNOWLEDGE_CANDIDATE_GENERATION_BUDGET", "KNOWLEDGE_FUSION_BUDGET"]) expect(query).not.toContain(retired);
    const planner = fs.readFileSync(new URL("../lib/knowledge/knowledge-coverage-planner.ts", import.meta.url), "utf8");
    expect(planner).not.toContain("planKnowledgeCoverage(");
    expect(planner).not.toContain("classifyModel(");
    for (const source of [facade, legacy]) {
      expect(source.statements.some(node => ts.isExportDeclaration(node) && !node.exportClause)).toBe(false);
    }
  });
});
