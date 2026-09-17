// shared/tool-categories.ts
//
// Single source of truth for built-in tool categorization.
//
// Every tool the engine registers (excluding plugin-contributed ones) MUST
// belong to exactly one of the three arrays below. A startup assertion enforces
// this — if a new tool is added without categorization, the engine refuses to
// boot with an error pointing here.
//
// Categories:
//   CORE     — Removing breaks the model. Never user-toggleable, never in UI.
//   STANDARD — Always-on built-in. Not in UI. Move to OPTIONAL to expose a toggle.
//   OPTIONAL — User-toggleable in AgentTab → Tools section. Some may default off.
//   GLOBAL   — Built-in, but governed by a global high-permission setting page
//              rather than per-agent tool toggles.
//   LEGACY_INTERNAL — Known retired/internal transports kept for replay or direct
//                     unit coverage. Never registered into the Agent tool surface.
//
// Plugin-contributed tools (flagged with _pluginId) are NOT part of this
// categorization. Plugin lifecycle is managed by PluginsTab. A plugin-backed
// product feature may expose a synthetic OPTIONAL category when it needs a
// per-agent switch; concrete plugin tools then implement runtime availability.

export const CORE_TOOL_NAMES = [
  "read", "write", "edit", "exec_command", "write_stdin", "grep", "find", "ls",
  "search_memory", "pin_memory", "unpin_memory",
  "web_search",
];

export const STANDARD_TOOL_NAMES = [
  "web_fetch",
  "todo_write",
  "notify",
  "stage_files",
  "file",
  "materialize",
  "subagent",
  "subagent_reply",
  "subagent_close",
  "channel",
  "record_experience",
  "recall_experience",
  "tenet_propose",
  "check_pending_tasks",
  "current_status",
  "ask_user",
  "session_folders",
  "stop_task",
  "hana_card_guide",
  "show_card",
  "loop_control",
  "knowledge_search",
  "knowledge_read",
  "knowledge_outline",
  "knowledge_grep",
  "knowledge_manage",
];

export const GLOBAL_TOOL_NAMES = [
  "computer",
];

export const LEGACY_INTERNAL_TOOL_NAMES = [
  "terminal",
  "dm",
];

export const OPTIONAL_TOOL_NAMES = [
  "automation",
  "ast_edit",
  "ast_grep",
  "checkpoint",
  "goal",
  "context_notes",
  "beautify",
  "browser",
  "lsp",
  "install_skill",
  "learn_lesson",
  "office",
  "rewind",
  "run_code",
  "security_scan",
  "session",
  "update_settings",
  "workflow",
];

export const PLUGIN_BACKED_OPTIONAL_TOOL_IDS = {
  beautify: "beautify",
  office: "office",
};

/**
 * The only first-party tools a session keeps in its cacheable tool prefix.
 *
 * Product rule (2026-09): the resident surface is exactly the four original
 * Pi coding-agent tools — read, write, edit, exec_command. Everything else
 * categorized below defers into the on-demand catalog and is reached through
 * mcp_search_tools / mcp_describe_tool / mcp_call.
 */
export const RESIDENT_CORE_TOOL_NAMES = [
  "read",
  "write",
  "edit",
  "exec_command",
];

/**
 * Permission contracts for deferred first-party tools that own no
 * `sessionPermission` resolver (the host permission classifier adjudicates
 * them on the direct surface). A deferred invocation authorizes through the
 * target's own contract, so each of these must reproduce what the host
 * classifier would decide — never wider. Values:
 *
 *   "read"    — allow in every mode; reserved for INFORMATION_TOOLS members
 *               (core/session-permission-mode.ts) whose direct path always
 *               allows.
 *   "execute" — allow in operate, blocked in read-only, prompt/review
 *               otherwise; matches the direct path for these tools or is one
 *               step stricter (fail-closed).
 *   "file" / "session-folders" — argument-aware: their read-only sub-actions
 *               map to read, everything else to execute, mirroring
 *               classifyFileAction / classifySessionFoldersAction.
 *
 * A deferred candidate missing from this map AND lacking its own resolver
 * stays resident (engine falls back with a warning) — never deferred blind.
 */
export const FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS = {
  grep: "read",
  find: "read",
  ls: "read",
  web_search: "read",
  web_fetch: "read",
  current_status: "read",
  search_memory: "read",
  write_stdin: "execute",
  computer: "execute",
  session: "execute",
  workflow: "execute",
  file: "file",
  "session_folders": "session-folders",
};
// Mirrors FILE_READ_ACTIONS / SESSION_COLLAB_READ_ACTIONS in
// core/session-permission-mode.ts. Kept as literal sets with a drift note
// rather than an import so shared/ stays dependency-free; the startup
// assertion cannot see these, so changes there must be mirrored here.
const FILE_TOOL_READ_ACTIONS = new Set(["stat"]);
const SESSION_FOLDERS_READ_ACTIONS = new Set(["list"]);

/**
 * The synthetic invocation descriptor for a deferred first-party tool without
 * its own resolver. `params` may be undefined for a static contract. Returns
 * null when the tool's contract kind is argument-aware but the arguments are
 * not a plain record — the caller must then fail closed.
 */
export function firstPartyDeferredInvocation(name: string, params?: unknown) {
  const kind = FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS[name];
  if (!kind) return null;
  if (kind === "read") {
    return { action: "read", kind: "read", capability: `${name}.read` };
  }
  if (kind === "execute") {
    return { action: "execute", kind: "review", capability: `${name}.execute` };
  }
  if (typeof params !== "object" || params === null || Array.isArray(params)) return null;
  const action = typeof (params as Record<string, unknown>).action === "string"
    ? (params as Record<string, unknown>).action as string
    : "";
  if (kind === "file") {
    return FILE_TOOL_READ_ACTIONS.has(action)
      ? { action: "read", kind: "read", capability: `${name}.read` }
      : { action: "execute", kind: "review", capability: `${name}.execute` };
  }
  if (kind === "session-folders") {
    return SESSION_FOLDERS_READ_ACTIONS.has(action)
      ? { action: "read", kind: "read", capability: `${name}.read` }
      : { action: "execute", kind: "review", capability: `${name}.execute` };
  }
  return null;
}

/**
 * On-demand (deferred) first-party tools: every categorized built-in except
 * the resident four and the retired internal transports. Computed rather than
 * hand-listed so the product rule stays "only read/write/edit/exec_command
 * remain resident" when new tools are added — assertAllToolsCategorized still
 * forces every new name into a category, and this derivation then defers it
 * by default.
 */
export const ONDEMAND_CORE_TOOL_NAMES = uniqueToolNames(
  [
    ...CORE_TOOL_NAMES,
    ...STANDARD_TOOL_NAMES,
    ...GLOBAL_TOOL_NAMES,
    ...OPTIONAL_TOOL_NAMES,
  ].filter((name) => (
    !RESIDENT_CORE_TOOL_NAMES.includes(name)
    && !LEGACY_INTERNAL_TOOL_NAMES.includes(name)
  )),
);

/**
 * Built-ins whose invocation boundary is enforced by an older host-owned
 * gateway instead of a tool-owned `sessionPermission.resolveInvocation`.
 *
 * Keep this list small and explicit. Adding a name here is a security decision:
 * the named implementation must already derive its authority from the host
 * sandbox, ResourceIO, or an internal capability gate.
 */
export const BUILT_IN_PERMISSION_GATEWAY_TOOL_NAMES = [
  // PI filesystem primitives are constrained by the session ResourceIO/sandbox.
  "read", "write", "edit", "grep", "find", "ls",
  // Existing read/network surfaces still use the host permission classifier.
  "search_memory", "web_search", "web_fetch",
  // These tools own a deeper action/path/session gate at execution time.
  "file", "current_status", "session_folders", "computer",
  "session", "workflow",
];

const OPTIONAL_TOOL_NAMES_SET = new Set(OPTIONAL_TOOL_NAMES);

/**
 * Default-off subset of OPTIONAL_TOOL_NAMES. Applied when agent config has no
 * `tools.disabled` field (i.e., user has never touched tool settings). Both
 * fresh agents and agents upgrading from a pre-feature version hit this path.
 *
 * Must be a subset of OPTIONAL_TOOL_NAMES. The frontend AgentTab keeps a local
 * copy for display defaults; tests/optional-tool-names-drift.test.ts guards the
 * two from drifting.
 *
 * Rationale:
 *   workflow        — deterministic multi-agent orchestration; a heavy fan-out
 *                     capability, opt-in per agent until it has baked.
 *   (beautify 已于 0.375.x 毕业为默认开启。)
 */
export const DEFAULT_DISABLED_TOOL_NAMES = ["workflow"];

export function uniqueToolNames(names) {
  const seen = new Set();
  const result = [];
  for (const name of names || []) {
    if (typeof name !== "string" || name.length === 0 || seen.has(name)) continue;
    seen.add(name);
    result.push(name);
  }
  return result;
}

/**
 * Settings needs a stable per-agent tool configuration surface even when an
 * agent is only config-loaded and its runtime tools have not been initialized.
 * Runtime tool names are preserved for compatibility, while built-in optional
 * categories are exposed from the central whitelist. Plugin-backed optional
 * categories only appear when their plugin tools are actually registered.
 *
 * @param {string[]} runtimeToolNames
 * @param {{ pluginTools?: Array<{ _pluginId?: string }> }} [options]
 * @returns {string[]}
 */
export function computeSettingsAvailableToolNames(runtimeToolNames, options: { pluginTools?: Array<{ _pluginId?: string }> } = {}) {
  const result = new Set(uniqueToolNames(runtimeToolNames));
  const pluginTools = Array.isArray(options.pluginTools) ? options.pluginTools : [];
  for (const name of OPTIONAL_TOOL_NAMES) {
    const pluginId = PLUGIN_BACKED_OPTIONAL_TOOL_IDS[name];
    if (pluginId && !pluginTools.some((tool) => tool?._pluginId === pluginId)) continue;
    result.add(name);
  }
  return [...result];
}

/**
 * Startup-time invariant: every built-in tool the engine composes MUST be
 * explicitly categorized. Throwing here always means a developer added a tool
 * without categorizing it. The fix is always: open this file and categorize it.
 *
 * Caller passes already-filtered names (plugin tools excluded by caller).
 *
 * @param {string[]} actualToolNames
 * @throws {Error} if any tool is uncategorized
 */
export function assertAllToolsCategorized(actualToolNames) {
  const categorized = new Set([
    ...CORE_TOOL_NAMES,
    ...STANDARD_TOOL_NAMES,
    ...GLOBAL_TOOL_NAMES,
    ...LEGACY_INTERNAL_TOOL_NAMES,
    ...OPTIONAL_TOOL_NAMES,
  ]);
  const missing = actualToolNames.filter((n) => !categorized.has(n));
  if (missing.length > 0) {
    throw new Error(
      `Tools not categorized in shared/tool-categories.js: ${missing.join(", ")}.\n` +
      `Every built-in tool must be explicitly labeled as core / standard / optional. ` +
      `See the header of shared/tool-categories.js for the decision rules.`
    );
  }
}

function hasOwnDataPluginId(tool) {
  if (!tool || (typeof tool !== "object" && typeof tool !== "function")) return false;
  try {
    const descriptor = Object.getOwnPropertyDescriptor(tool, "_pluginId");
    return !!descriptor
      && Object.prototype.hasOwnProperty.call(descriptor, "value")
      && typeof descriptor.value === "string"
      && descriptor.value.trim().length > 0;
  } catch {
    return false;
  }
}

function hasOwnInvocationPermissionResolver(tool) {
  if (!tool || (typeof tool !== "object" && typeof tool !== "function")) return false;
  try {
    const permissionDescriptor = Object.getOwnPropertyDescriptor(tool, "sessionPermission");
    if (
      !permissionDescriptor
      || !Object.prototype.hasOwnProperty.call(permissionDescriptor, "value")
      || !permissionDescriptor.value
      || typeof permissionDescriptor.value !== "object"
    ) {
      return false;
    }
    const resolverDescriptor = Object.getOwnPropertyDescriptor(
      permissionDescriptor.value,
      "resolveInvocation",
    );
    return !!resolverDescriptor
      && Object.prototype.hasOwnProperty.call(resolverDescriptor, "value")
      && typeof resolverDescriptor.value === "function";
  } catch {
    return false;
  }
}

/**
 * Startup invariant for the permission catalog. Every non-plugin built-in must
 * either declare a synchronous tool-owned invocation resolver or be named in
 * the explicit host-gateway list above. Plugin tools are intentionally skipped:
 * a plugin without a declaration remains review-required at runtime.
 *
 * @param {Array<object>} actualTools
 * @throws {Error} if a built-in has no permission boundary
 */
export function assertAllBuiltInToolsPermissionCovered(actualTools) {
  const gatewayNames = new Set(BUILT_IN_PERMISSION_GATEWAY_TOOL_NAMES);
  const missing = uniqueToolNames((actualTools || [])
    .filter((tool) => !hasOwnDataPluginId(tool))
    .filter((tool) => {
      const name = typeof tool?.name === "string" ? tool.name : "";
      return name && !gatewayNames.has(name) && !hasOwnInvocationPermissionResolver(tool);
    })
    .map((tool) => tool.name));

  if (missing.length > 0) {
    throw new Error(
      `Built-in tools missing invocation permission coverage: ${missing.join(", ")}.\n`
      + "Add a tool-owned sessionPermission.resolveInvocation descriptor, or explicitly document its host gateway in shared/tool-categories.js.",
    );
  }
}

/**
 * Startup invariant for the on-demand split. Throwing here always means a
 * developer broke one of these rules in this file:
 *   - the resident set is exactly four tools, all core-categorized;
 *   - the resident set never overlaps the on-demand set;
 *   - every synthetic permission contract names an on-demand tool.
 * The fix is always here — never at the call site.
 *
 * @throws {Error} on any violation
 */
export function assertOnDemandCoreToolNamesSound() {
  const coreNames = new Set(CORE_TOOL_NAMES);
  const problems = [];
  for (const name of RESIDENT_CORE_TOOL_NAMES) {
    if (!coreNames.has(name)) problems.push(`${name}: resident set must only contain CORE tools`);
  }
  if (problems.length === 0) {
    const onDemand = new Set(ONDEMAND_CORE_TOOL_NAMES);
    for (const name of RESIDENT_CORE_TOOL_NAMES) {
      if (onDemand.has(name)) problems.push(`${name}: resident and on-demand sets overlap`);
    }
    for (const name of Object.keys(FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS)) {
      if (!onDemand.has(name)) {
        problems.push(`${name}: synthetic contract exists but the tool is resident or unknown`);
      }
    }
  }
  if (problems.length > 0) {
    throw new Error(
      `The on-demand split in shared/tool-categories.ts is unsound:\n  - ${problems.join("\n  - ")}`,
    );
  }
}

/**
 * Compute the final tool name list for a newly created session.
 *
 * Rule: remove from allNames any name that is BOTH in the disabled list AND
 * in OPTIONAL_TOOL_NAMES. Core/standard tools are untouchable even if the
 * disabled list has been tampered with (runtime second-line defense).
 *
 * @param {string[]} allNames
 * @param {string[]} disabled
 * @param {{ extraDisabled?: string[] }} [options]
 * @returns {string[]} filtered tool names, order preserved from allNames
 */
export function computeToolSnapshot(allNames, disabled, options: { extraDisabled?: string[] } = {}) {
  const effectivelyDisabled = new Set(
    (disabled || []).filter((n) => OPTIONAL_TOOL_NAMES_SET.has(n))
  );
  const extraDisabled = new Set(
    (options.extraDisabled || []).filter((n) => typeof n === "string" && n)
  );
  return uniqueToolNames(allNames)
    .filter((n) => !effectivelyDisabled.has(n) && !extraDisabled.has(n));
}
