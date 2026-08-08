import { describe, expect, it } from "vitest";
import {
  inferOllamaModelMetadata,
  enrichOllamaModelMetadata,
} from "../shared/ollama-model-metadata.ts";

describe("inferOllamaModelMetadata", () => {
  describe("vision (model-name heuristic)", () => {
    it("recognizes llava", () => {
      expect(inferOllamaModelMetadata("ollama", "llava:latest")).toEqual({ image: true });
    });

    it("recognizes gemma3", () => {
      expect(inferOllamaModelMetadata("ollama", "gemma3:4b")).toEqual({ image: true });
    });

    it("recognizes gemma4 (gemma[3-9] range)", () => {
      expect(inferOllamaModelMetadata("ollama", "gemma4:12b-nvfp4")).toEqual({ image: true });
    });

    it("recognizes minicpm-v", () => {
      expect(inferOllamaModelMetadata("ollama", "minicpm-v:8b")?.image).toBe(true);
    });

    it("does not recognize plain text models", () => {
      expect(inferOllamaModelMetadata("ollama", "llama3.1:8b")).toBeNull();
    });

    it("does not recognize gemma2 (below range)", () => {
      expect(inferOllamaModelMetadata("ollama", "gemma2:9b")).toBeNull();
    });
  });

  describe("capabilities (from /api/tags)", () => {
    it("infers toolUse when capabilities include tools", () => {
      const result = inferOllamaModelMetadata("ollama", {
        id: "gemma4:12b-nvfp4",
        _ollamaCapabilities: ["completion", "tools", "thinking"],
      });
      expect(result?.toolUse).toEqual({
        supportsTools: true,
        dialect: "openai",
        toolResultFormat: "message",
      });
    });

    it("infers reasoning when capabilities include thinking", () => {
      const result = inferOllamaModelMetadata("ollama", {
        id: "deepseek-r1:8b",
        _ollamaCapabilities: ["completion", "thinking"],
      });
      expect(result?.reasoning).toBe(true);
    });

    it("infers both toolUse and reasoning when both capabilities present", () => {
      const result = inferOllamaModelMetadata("ollama", {
        id: "gemma4:12b-nvfp4",
        _ollamaCapabilities: ["completion", "tools", "thinking"],
      });
      expect(result?.toolUse).toBeDefined();
      expect(result?.reasoning).toBe(true);
      expect(result?.image).toBe(true); // gemma4 also matches vision pattern
    });

    it("infers nothing for completion-only capabilities", () => {
      const result = inferOllamaModelMetadata("ollama", {
        id: "some-model:7b",
        _ollamaCapabilities: ["completion"],
      });
      expect(result).toBeNull();
    });

    it("reads capabilities from model.capabilities as fallback", () => {
      const result = inferOllamaModelMetadata("ollama", {
        id: "test-model:7b",
        capabilities: ["tools"],
      });
      expect(result?.toolUse).toBeDefined();
    });
  });

  it("returns null for non-ollama providers", () => {
    expect(inferOllamaModelMetadata("openai", "llava:latest")).toBeNull();
  });

  it("returns null for empty model id", () => {
    expect(inferOllamaModelMetadata("ollama", "")).toBeNull();
  });
});

describe("enrichOllamaModelMetadata", () => {
  it("enriches a plain string model id with vision", () => {
    const result = enrichOllamaModelMetadata("ollama", "llava:latest");
    expect(result).toEqual({ id: "llava:latest", image: true });
  });

  it("enriches an object model with capabilities-based toolUse", () => {
    const model = {
      id: "gemma4:12b-nvfp4",
      context: 262144,
      _ollamaCapabilities: ["completion", "tools", "thinking"],
    };
    const result = enrichOllamaModelMetadata("ollama", model) as Record<string, any>;
    expect(result.toolUse).toEqual({
      supportsTools: true,
      dialect: "openai",
      toolResultFormat: "message",
    });
    expect(result.reasoning).toBe(true);
    expect(result.image).toBe(true);
    expect(result.context).toBe(262144);
  });

  it("does not overwrite explicit user-set toolUse", () => {
    const model = {
      id: "gemma4:12b-nvfp4",
      toolUse: { supportsTools: false, dialect: "none", toolResultFormat: "message" },
      _ollamaCapabilities: ["tools"],
    };
    const result = enrichOllamaModelMetadata("ollama", model) as Record<string, any>;
    expect(result.toolUse).toEqual({
      supportsTools: false,
      dialect: "none",
      toolResultFormat: "message",
    });
  });

  it("does not overwrite explicit user-set reasoning", () => {
    const model = {
      id: "deepseek-r1:8b",
      reasoning: false,
      _ollamaCapabilities: ["thinking"],
    };
    const result = enrichOllamaModelMetadata("ollama", model) as Record<string, any>;
    expect(result.reasoning).toBe(false);
  });

  it("does not overwrite explicit user-set image", () => {
    const model = {
      id: "llava:latest",
      image: false,
    };
    const result = enrichOllamaModelMetadata("ollama", model) as Record<string, any>;
    expect(result.image).toBe(false);
  });

  it("returns model unchanged when nothing can be inferred", () => {
    const model = { id: "llama3.1:8b", context: 131072 };
    const result = enrichOllamaModelMetadata("ollama", model);
    expect(result).toEqual(model);
  });
});
