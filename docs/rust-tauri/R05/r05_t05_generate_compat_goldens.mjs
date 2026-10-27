#!/usr/bin/env node
/**
 * R05-T05: generate the provider-compat golden fixtures
 * (rust/crates/lingxi-adapters/tests/golden-compat/*.json) by running the
 * INCUMBENT TypeScript modules (Node 24 type stripping) on a registered
 * case table. The Rust port (models/compat.rs) is pinned to these fixtures
 * byte-for-byte by tests/r05_t05_compat.rs — any drift between the port and
 * the incumbent breaks the suite loudly.
 *
 * Per case the fixture records:
 *   - model: the TS model object verbatim (provider/id/baseUrl/api +
 *     reasoning/maxTokens/contextWindow/quirks/thinkingLevelMap/video +
 *     compat {thinkingFormat, reasoningProfile, cacheControlFormat,
 *     outputIncludesThinking}).
 *   - payload / options: the module-apply inputs verbatim.
 *   - expected.thinkingFormat / reasoningProfile: the incumbent
 *     shared/model-capabilities.ts derivation results.
 *   - expected.matched: the first-match dispatch decision over the
 *     incumbent PROVIDER_MODULES order (the two non-ported modules are
 *     included in the dispatch check; a case that would dispatch to one of
 *     them is a GENERATOR failure — the port's registry must decide
 *     identically on every registered case).
 *   - expected.result XOR expected.error: the incumbent module apply()
 *     output, or the thrown message verbatim.
 *
 * Regenerate: node docs/rust-tauri/R05/r05_t05_generate_compat_goldens.mjs
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../../..");
const OUT_DIR = path.join(REPO, "rust/crates/lingxi-adapters/tests/golden-compat");

const compatDir = path.join(REPO, "core/provider-compat");
const modules = {
  "deepseek-responses": await import(path.join(compatDir, "deepseek-responses.ts")),
  deepseek: await import(path.join(compatDir, "deepseek.ts")),
  kimi: await import(path.join(compatDir, "kimi.ts")),
  mimo: await import(path.join(compatDir, "mimo.ts")),
  qwen: await import(path.join(compatDir, "qwen.ts")),
  zhipu: await import(path.join(compatDir, "zhipu.ts")),
  volcengine: await import(path.join(compatDir, "volcengine.ts")),
  longcat: await import(path.join(compatDir, "longcat.ts")),
  agnes: await import(path.join(compatDir, "agnes.ts")),
  "openai-input-audio": await import(path.join(compatDir, "openai-input-audio.ts")),
  openrouter: await import(path.join(compatDir, "openrouter.ts")),
  anthropic: await import(path.join(compatDir, "anthropic.ts")),
  "codex-responses": await import(path.join(compatDir, "codex-responses.ts")),
  ollama: await import(path.join(compatDir, "ollama.ts")),
};
const { getThinkingFormat, getReasoningProfile } = await import(
  path.join(REPO, "shared/model-capabilities.ts")
);

// The incumbent PROVIDER_MODULES order (core/provider-compat.ts).
const DISPATCH_ORDER = [
  "deepseek-responses",
  "deepseek",
  "kimi",
  "mimo",
  "qwen",
  "zhipu",
  "volcengine",
  "longcat",
  "agnes",
  "openai-input-audio",
  "openrouter",
  "anthropic",
  "codex-responses",
  "ollama",
];
// The modules the Rust port serves (longcat / openai-input-audio are
// registered non-ported scope decisions — R05_INTERFACE_EVOLUTION §25).
const PORTED = new Set(DISPATCH_ORDER.filter((m) => m !== "longcat" && m !== "openai-input-audio"));

// ── shared model fixtures ──────────────────────────────────────────────────
const DEEPSEEK = { provider: "deepseek", baseUrl: "https://api.deepseek.com" };
const DEEPSEEK_REASONER = { ...DEEPSEEK, id: "deepseek-reasoner", reasoning: true };
const DEEPSEEK_V4 = { ...DEEPSEEK, id: "deepseek-v4.2", reasoning: true };
const KIMI = {
  provider: "kimi-coding",
  id: "kimi-for-coding",
  baseUrl: "https://api.kimi.com/coding/v1",
  api: "openai-completions",
  reasoning: true,
};
const MIMO = {
  provider: "mimo",
  id: "mimo-v7-pro",
  baseUrl: "https://xiaomimimo.com/v1",
  api: "openai-completions",
  reasoning: true,
};
const ZHIPU = {
  provider: "zhipu",
  id: "glm-4.7",
  baseUrl: "https://open.bigmodel.cn/api/paas/v4",
  api: "openai-completions",
  reasoning: true,
};
const VOLCENGINE = {
  provider: "volcengine",
  id: "doubao-seed-1-8",
  baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
  api: "openai-completions",
  reasoning: true,
};
const OPENROUTER_ADAPTIVE = {
  provider: "openrouter",
  id: "anthropic/claude-opus-5",
  baseUrl: "https://openrouter.ai/api/v1",
  api: "openai-completions",
  reasoning: true,
};
const ANTHROPIC = {
  provider: "anthropic",
  id: "claude-sonnet-5",
  baseUrl: "https://api.anthropic.com",
  api: "anthropic-messages",
};
const OLLAMA = {
  provider: "ollama",
  id: "qwen3",
  baseUrl: "http://127.0.0.1:11434/v1",
  api: "openai-completions",
};

/** case(module-slug, case-slug, model, payload, options) — options.mode
 * defaults to "chat" explicitly (production's CompatOptions::for_operation
 * always sets the mode; a MISSING mode is a different TS shape — qwen's
 * three-way mode gate treats it as neither chat nor utility). */
const C = (module, slug, model, payload, options = {}) => ({
  module,
  slug,
  model,
  payload,
  options: { mode: "chat", ...options },
});

const CASES = [
  // ── deepseek-responses (official DeepSeek over openai-responses) ─────────
  C(
    "deepseek-responses",
    "disable-level-forces-none-effort",
    { ...DEEPSEEK_V4, api: "openai-responses" },
    { model: "deepseek-v4.2", input: [], reasoning: { effort: "high" }, max_tokens: 4096 },
    { reasoningLevel: "off" },
  ),
  C(
    "deepseek-responses",
    "missing-budget-fills-default",
    { ...DEEPSEEK_V4, api: "openai-responses" },
    { model: "deepseek-v4.2", input: [] },
  ),
  C(
    "deepseek-responses",
    "effort-minimal-translates-low",
    { ...DEEPSEEK_V4, api: "openai-responses" },
    { model: "deepseek-v4.2", input: [], reasoning: { effort: "minimal", summary: "auto" }, max_output_tokens: 2048 },
  ),
  C(
    "deepseek-responses",
    "clean-payload-passes-through",
    { ...DEEPSEEK_V4, api: "openai-responses" },
    { model: "deepseek-v4.2", input: [], reasoning: { effort: "high" }, max_output_tokens: 2048 },
  ),

  // ── deepseek (official DeepSeek over openai-completions) ─────────────────
  C(
    "deepseek",
    "max-completion-tokens-renames",
    { ...DEEPSEEK, id: "deepseek-chat", api: "openai-completions" },
    { model: "deepseek-chat", messages: [], max_completion_tokens: 2048 },
  ),
  C(
    "deepseek",
    "off-level-disables-and-strips",
    { ...DEEPSEEK_REASONER, api: "openai-completions" },
    {
      model: "deepseek-reasoner",
      messages: [{ role: "assistant", content: "a", reasoning_content: "trace" }],
      reasoning_effort: "high",
    },
    { reasoningLevel: "off" },
  ),
  C(
    "deepseek",
    "level-enables-and-keeps-budget",
    { ...DEEPSEEK_REASONER, api: "openai-completions" },
    { model: "deepseek-reasoner", messages: [], tool_choice: "auto", max_tokens: 8192 },
    { reasoningLevel: "low" },
  ),
  C(
    "deepseek",
    "missing-budget-fills-default",
    { ...DEEPSEEK_REASONER, api: "openai-completions" },
    { model: "deepseek-reasoner", messages: [] },
    { reasoningLevel: "high" },
  ),
  C(
    "deepseek",
    "utility-mode-disables",
    { ...DEEPSEEK_REASONER, api: "openai-completions" },
    { model: "deepseek-reasoner", messages: [], reasoning_effort: "high" },
    { mode: "utility" },
  ),
  C(
    "deepseek",
    "anthropic-profile-effort-and-budget",
    { ...DEEPSEEK_V4, api: "anthropic-messages" },
    {
      model: "deepseek-v4.2",
      messages: [],
      thinking: { type: "enabled", budget_tokens: 5000 },
      reasoning_effort: "low",
    },
    { reasoningLevel: "medium" },
  ),

  // ── kimi ─────────────────────────────────────────────────────────────────
  C(
    "kimi",
    "level-enables-with-keep-all",
    KIMI,
    { model: "kimi-for-coding", messages: [] },
    { reasoningLevel: "low" },
  ),
  C(
    "kimi",
    "off-level-disables-and-strips",
    KIMI,
    {
      model: "kimi-for-coding",
      messages: [{ role: "assistant", content: "x", reasoning_content: "trace" }],
      thinking: { type: "enabled" },
    },
    { reasoningLevel: "off" },
  ),
  C(
    "kimi",
    "max-tokens-renames-and-keep-preserved",
    KIMI,
    { model: "kimi-for-coding", messages: [], max_tokens: 4096, thinking: { type: "enabled", keep: "thread" } },
  ),
  C(
    "kimi",
    "utility-mode-pins-temperature-and-disables",
    KIMI,
    { model: "kimi-for-coding", messages: [] },
    { mode: "utility" },
  ),
  C(
    "kimi",
    "k3-omits-temperature",
    { ...KIMI, id: "k3" },
    { model: "k3", messages: [], temperature: 0.7, thinking: { type: "enabled" } },
  ),
  C(
    "kimi",
    "mfjs-root-anyof-folds-into-description",
    KIMI,
    {
      model: "kimi-for-coding",
      messages: [],
      thinking: { type: "enabled" },
      tools: [
        {
          type: "function",
          function: {
            name: "f",
            parameters: {
              type: "object",
              anyOf: [{ required: ["a"] }, { required: ["b"] }],
              properties: { a: { type: "string" }, b: { type: "string" } },
            },
          },
        },
      ],
    },
  ),
  C(
    "kimi",
    "thinking-level-map-overrides-the-default-vocabulary",
    { ...KIMI, thinkingLevelMap: { xhigh: "max", low: null } },
    { model: "kimi-for-coding", messages: [] },
    { reasoningLevel: "xhigh" },
  ),

  // ── mimo ─────────────────────────────────────────────────────────────────
  C(
    "mimo",
    "declared-reasoning-enables-kwargs",
    MIMO,
    { model: "mimo-v7-pro", messages: [] },
  ),
  C(
    "mimo",
    "off-level-disables-and-strips",
    MIMO,
    {
      model: "mimo-v7-pro",
      messages: [{ role: "assistant", content: "a", reasoning_content: "trace" }],
      chat_template_kwargs: { enable_thinking: true, preserve_thinking: true },
    },
    { reasoningLevel: "off" },
  ),
  C(
    "mimo",
    "utility-mode-disables",
    MIMO,
    { model: "mimo-v7-pro", messages: [] },
    { mode: "utility" },
  ),
  C(
    "mimo",
    "no-thinking-signals-pass-through",
    { provider: "mimo", id: "mimo-v7-pro", baseUrl: "https://xiaomimimo.com/v1", api: "openai-completions" },
    { model: "mimo-v7-pro", messages: [] },
  ),

  // ── qwen ─────────────────────────────────────────────────────────────────
  C(
    "qwen",
    "off-level-forces-enable-thinking-false",
    { provider: "custom", id: "qwen3-32b", baseUrl: "https://dashscope.aliyuncs.com/v1", api: "openai-completions", quirks: ["enable_thinking"] },
    { model: "qwen3-32b", messages: [], enable_thinking: true },
    { reasoningLevel: "off" },
  ),
  C(
    "qwen",
    "chat-level-passes-through",
    { provider: "custom", id: "qwen3-32b", baseUrl: "https://dashscope.aliyuncs.com/v1", api: "openai-completions", quirks: ["enable_thinking"] },
    { model: "qwen3-32b", messages: [] },
    { reasoningLevel: "high" },
  ),
  C(
    "qwen",
    "utility-mode-adds-the-switch",
    { provider: "custom", id: "qwen3-32b", baseUrl: "https://dashscope.aliyuncs.com/v1", api: "openai-completions", quirks: ["enable_thinking"] },
    { model: "qwen3-32b", messages: [] },
    { mode: "utility" },
  ),
  C(
    "qwen",
    "dashscope-video-route-matches-without-quirks",
    { provider: "dashscope", id: "qwen3-vl-plus", baseUrl: "https://dashscope.aliyuncs.com/v1", api: "openai-completions", video: true },
    { model: "qwen3-vl-plus", messages: [] },
    { mode: "utility" },
  ),

  // ── zhipu ────────────────────────────────────────────────────────────────
  C(
    "zhipu",
    "declared-reasoning-enables-and-strips-store",
    ZHIPU,
    { model: "glm-4.7", messages: [], store: true, stream_options: { include_usage: true } },
  ),
  C(
    "zhipu",
    "off-level-disables-and-strips",
    ZHIPU,
    {
      model: "glm-4.7",
      messages: [{ role: "assistant", content: "a", reasoning_content: "trace" }],
    },
    { reasoningLevel: "off" },
  ),
  C(
    "zhipu",
    "clear-replay-policy-round-trips",
    ZHIPU,
    {
      model: "glm-4.7",
      messages: [
        {
          role: "assistant",
          content: "a",
          reasoning_content: "trace",
          tool_calls: [{ id: "c1", type: "function", function: { name: "f", arguments: "{}" } }],
        },
      ],
      thinking: { type: "enabled", clear_thinking: true },
    },
  ),
  C(
    "zhipu",
    "preserve-marks-clear-false-with-tool-history",
    ZHIPU,
    {
      model: "glm-4.7",
      messages: [
        {
          role: "assistant",
          content: null,
          tool_calls: [{ id: "c1", type: "function", function: { name: "f", arguments: "{}" } }],
        },
      ],
    },
  ),
  C(
    "zhipu",
    "opencode-go-never-emits-clear-thinking",
    {
      provider: "opencode-go",
      id: "glm-4.7",
      baseUrl: "https://opencode.ai/zen/go/v1",
      api: "openai-completions",
      reasoning: true,
      // The opencode-go endpoint reaches the zhipu module only through the
      // DECLARED thinking format (the incumbent matcher has no opencode leg).
      compat: { thinkingFormat: "zhipu" },
    },
    {
      model: "glm-4.7",
      messages: [
        {
          role: "assistant",
          content: "a",
          tool_calls: [{ id: "c1", type: "function", function: { name: "f", arguments: "{}" } }],
        },
      ],
      thinking: { type: "enabled", clear_thinking: true },
    },
    { reasoningReplay: "clear" },
  ),
  C(
    "zhipu",
    "tools-strict-stripped-and-max-tokens-renamed",
    ZHIPU,
    {
      model: "glm-4.7",
      messages: [],
      max_completion_tokens: 1024,
      tools: [
        { type: "function", strict: true, function: { name: "f", strict: true, parameters: { type: "object" } } },
      ],
    },
  ),

  // ── volcengine ───────────────────────────────────────────────────────────
  C(
    "volcengine",
    "level-enables-with-mapped-effort",
    VOLCENGINE,
    { model: "doubao-seed-1-8", messages: [] },
    { reasoningLevel: "high" },
  ),
  C(
    "volcengine",
    "auto-level-maps-medium",
    VOLCENGINE,
    { model: "doubao-seed-1-8", messages: [] },
    { reasoningLevel: "auto" },
  ),
  C(
    "volcengine",
    "off-level-disables-and-strips",
    VOLCENGINE,
    {
      model: "doubao-seed-1-8",
      messages: [{ role: "assistant", content: "a", reasoning_content: "trace" }],
      reasoning_effort: "high",
    },
    { reasoningLevel: "off" },
  ),
  C(
    "volcengine",
    "existing-effort-remapped-into-vocabulary",
    VOLCENGINE,
    { model: "doubao-seed-1-8", messages: [], reasoning_effort: "max" },
  ),

  // ── agnes ────────────────────────────────────────────────────────────────
  C(
    "agnes",
    "strips-thinking-fields-and-blocks",
    { provider: "agnes", id: "agnes-2-flash", baseUrl: "https://agnes-ai.com/v1", api: "openai-completions" },
    {
      model: "agnes-2-flash",
      messages: [
        {
          role: "assistant",
          content: [
            { type: "thinking", thinking: "hidden" },
            { type: "text", text: "hello" },
          ],
          reasoning_content: "trace",
        },
      ],
      thinking: { type: "enabled" },
      reasoning_effort: "low",
      chat_template_kwargs: { enable_thinking: true },
    },
  ),

  // ── openrouter ───────────────────────────────────────────────────────────
  C(
    "openrouter",
    "adaptive-verbosity-defaults-high",
    OPENROUTER_ADAPTIVE,
    { model: "anthropic/claude-opus-5", messages: [], thinking: { type: "enabled" }, reasoning_effort: "high" },
  ),
  C(
    "openrouter",
    "low-level-maps-low-and-reasoning-is-normalized",
    OPENROUTER_ADAPTIVE,
    { model: "anthropic/claude-opus-5", messages: [], reasoning: { effort: "low", max_tokens: 1000, exclude: true } },
    { reasoningLevel: "low" },
  ),
  C(
    "openrouter",
    "disable-level-refuses-loudly",
    OPENROUTER_ADAPTIVE,
    { model: "anthropic/claude-opus-5", messages: [] },
    { reasoningLevel: "off" },
  ),

  // ── anthropic ────────────────────────────────────────────────────────────
  C(
    "anthropic",
    "cache-control-marks-system-and-two-recent-users",
    ANTHROPIC,
    {
      model: "claude-sonnet-5",
      system: "you are helpful",
      messages: [
        { role: "user", content: "one" },
        { role: "assistant", content: "reply" },
        { role: "user", content: [{ type: "text", text: "two" }] },
        { role: "user", content: "three" },
      ],
      max_tokens: 16384,
    },
  ),
  C(
    "anthropic",
    "utility-mode-disables-thinking",
    ANTHROPIC,
    {
      model: "claude-sonnet-5",
      messages: [],
      thinking: { type: "enabled" },
      output_config: { effort: "high" },
      reasoning_effort: "high",
    },
    { mode: "utility" },
  ),
  C(
    "anthropic",
    "max-effort-raises-the-implicit-one-third-cap",
    { ...ANTHROPIC, id: "claude-opus-5", maxTokens: 96000 },
    { model: "claude-opus-5", messages: [{ role: "user", content: "hi" }], max_tokens: 32000 },
    { reasoningLevel: "max" },
  ),
  C(
    "anthropic",
    "user-sourced-budget-vetoes-the-raise",
    { ...ANTHROPIC, id: "claude-opus-5", maxTokens: 96000 },
    { model: "claude-opus-5", messages: [{ role: "user", content: "hi" }], max_tokens: 32000 },
    { reasoningLevel: "max", outputBudgetSource: "user" },
  ),

  // ── codex-responses ──────────────────────────────────────────────────────
  C(
    "codex-responses",
    "strips-budget-and-temperature",
    { provider: "openai-codex", id: "gpt-5.3-codex", baseUrl: "https://chatgpt.com/backend-api", api: "openai-codex-responses" },
    { model: "gpt-5.3-codex", input: [], max_output_tokens: 1024, max_tokens: 1024, temperature: 0.2, store: false },
  ),
  C(
    "codex-responses",
    "clean-payload-passes-through",
    { provider: "openai-codex", id: "gpt-5.3-codex", baseUrl: "https://chatgpt.com/backend-api", api: "openai-codex-responses" },
    { model: "gpt-5.3-codex", input: [], store: false },
  ),

  // ── ollama ───────────────────────────────────────────────────────────────
  C(
    "ollama",
    "response-format-bridge",
    OLLAMA,
    { model: "qwen3", messages: [] },
    { responseSchema: { type: "object", properties: { a: { type: "string" } } } },
  ),
  C(
    "ollama",
    "num-ctx-bridge-merges-options",
    { ...OLLAMA, contextWindow: 32768 },
    { model: "qwen3", messages: [], options: { temperature: 0.1 } },
  ),
  C(
    "ollama",
    "oversized-context-window-skips-num-ctx",
    { ...OLLAMA, contextWindow: 2097152 },
    { model: "qwen3", messages: [] },
  ),

  // ── dispatch honesty ─────────────────────────────────────────────────────
  C(
    "none",
    "unknown-provider-passes-through-unmatched",
    { provider: "acme", id: "acme-1", baseUrl: "https://api.acme.test/v1", api: "openai-completions" },
    { model: "acme-1", messages: [], reasoning_effort: "high" },
  ),
];

function main() {
  fs.rmSync(OUT_DIR, { recursive: true, force: true });
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const index = [];
  for (const testCase of CASES) {
    const { module: expectedModule, slug, model, payload, options } = testCase;
    // The dispatch decision over the incumbent registry order.
    let matched = null;
    for (const name of DISPATCH_ORDER) {
      if (modules[name].matches(model)) {
        matched = name;
        break;
      }
    }
    if (expectedModule === "none") {
      if (matched !== null) {
        throw new Error(`${slug}: expected no match, got ${matched}`);
      }
    } else {
      if (matched !== expectedModule) {
        throw new Error(`${slug}: expected ${expectedModule} to match first, got ${matched}`);
      }
      if (!PORTED.has(matched)) {
        throw new Error(`${slug}: dispatches to the non-ported module ${matched}`);
      }
    }
    const expected = {
      thinkingFormat: getThinkingFormat(model),
      reasoningProfile: getReasoningProfile(model),
      matched,
    };
    if (matched && PORTED.has(matched)) {
      try {
        const result = modules[matched].apply(structuredClone(payload), model, { ...options });
        expected.result = result;
      } catch (error) {
        expected.error = String(error && error.message ? error.message : error);
      }
    } else {
      expected.result = payload;
    }
    const file = `${matched ?? "none"}__${slug}.json`;
    const doc = {
      schema: "r05-t05-compat-golden/v1",
      module: matched,
      case: slug,
      model,
      payload,
      options,
      expected,
    };
    fs.writeFileSync(path.join(OUT_DIR, file), `${JSON.stringify(doc, null, 2)}\n`);
    index.push(file);
  }
  console.log(`wrote ${index.length} compat goldens to ${path.relative(REPO, OUT_DIR)}`);
  for (const file of index) console.log(`  ${file}`);
}

main();
