import fs from "node:fs";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { afterEach, describe, expect, it, vi } from "vitest";

const facadeUrl = new URL("../lib/knowledge/knowledge-context-injector.ts", import.meta.url);
const legacyUrl = new URL("../lib/knowledge/legacy/legacy-knowledge-context-injector.ts", import.meta.url);
afterEach(() => { vi.doUnmock("../lib/knowledge/legacy/legacy-knowledge-context-injector.ts"); vi.resetModules(); });

describe("知识上下文门面与旧详细编排的模块边界", () => {
  it("加载生产门面无需加载旧编排，门面不再暴露旧调查入口", async () => {
    vi.resetModules();
    vi.doMock("../lib/knowledge/legacy/legacy-knowledge-context-injector.ts", () => {
      throw new Error("生产门面不得加载旧调查编排");
    });
    const facade = await import("../lib/knowledge/knowledge-context-injector.ts");
    expect(facade.EvidencePacker).toBeTypeOf("function");
    expect(facade.renderKnowledgeContextBlock).toBeTypeOf("function");
    expect(facade.assembleKnowledgeEvidenceManifestEntries).toBeTypeOf("function");
    for (const name of ["buildKnowledgeContextInjection", "decomposeQuestion", "decomposeQuestionAdaptive", "expandQueries", "runGapAnalysis"]) {
      expect(name in facade).toBe(false);
    }
    expect(facade.resolveKnowledgeInjectionBudgetTokens({ contextWindow: 32000, maxOutput: 4096 })).toBe(27904);
  });

  it("旧入口仅在明确导入旧模块时可用，公共渲染和清单仍是门面的真实实现", async () => {
    const facade = await import("../lib/knowledge/knowledge-context-injector.ts");
    const legacy = await import("../lib/knowledge/legacy/legacy-knowledge-context-injector.ts");
    for (const name of ["buildKnowledgeContextInjection", "decomposeQuestion", "decomposeQuestionAdaptive", "expandQueries", "runGapAnalysis"] as const) {
      expect(legacy[name]).toBeTypeOf("function");
    }
    expect(legacy.renderKnowledgeContextBlock).toBe(facade.renderKnowledgeContextBlock);
    expect(legacy.assembleKnowledgeEvidenceManifestEntries).toBe(facade.assembleKnowledgeEvidenceManifestEntries);
    expect(legacy.knowledgeModeGuidance).toBe(facade.knowledgeModeGuidance);
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
    for (const source of [facade, legacy]) {
      expect(source.statements.some(node => ts.isExportDeclaration(node) && !node.exportClause)).toBe(false);
    }
  });
});
