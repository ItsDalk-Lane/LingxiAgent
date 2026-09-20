/**
 * Auto-compaction runtime wiring for a live session.
 *
 * Two pieces, both installed right after a session is created:
 *
 *   1. A proportional compaction reserve. The Pi SDK triggers auto-compaction
 *      when contextTokens exceeds `contextWindow - reserveTokens`, and a fixed
 *      reserve of 16384 tokens is meaningless once the model window is in the
 *      millions: the trigger point sits at 98%+ of the window, so in practice
 *      the session overflows before it ever compacts. The reserve is therefore
 *      derived from the live window as `max(16384, 20% of window)`, which puts
 *      the trigger at the FORCE line (80% of window) for windows >= 80K and
 *      keeps the absolute 16384-token headroom as the floor for small windows.
 *
 *   2. Mid-run compaction. The SDK only checks the threshold after a whole
 *      agentic run finishes and before a new prompt, so a single long tool loop
 *      can run far past the window with zero checks. The agent loop hands every
 *      turn to `prepareNextTurnWithContext` and adopts the context that call
 *      returns, which makes it the one supported seam where history can be
 *      swapped mid-run. The hook is wrapped (never replaced) so the SDK's own
 *      layer keeps working, and a compaction failure is swallowed: the run
 *      continues and the SDK's post-run check remains the backstop.
 *
 *   3. Dual thresholds. At every turn boundary the usage ratio is checked
 *      against two lines (shared/compaction-thresholds.ts):
 *
 *      - ASK line (50%): crossing it emits `compaction_suggested` to the
 *        desktop renderer, which shows a dialog asking the user whether to
 *        compact now. Core never blocks the run on the answer; re-prompts are
 *        throttled until usage grows by a further 5% of the window, and the
 *        prompt state resets after any compaction (a >=20% drop).
 *
 *      - FORCE line (80%): reaching it compacts unconditionally (the reserve
 *        derived in (1) makes the SDK's own check agree) and emits
 *        `compaction_auto` so the renderer can toast "context compacted".
 *
 * After a mid-run compaction the model would otherwise wake up to a summary
 * with no idea it was interrupted mid-task, so a hidden custom message is
 * appended telling it to keep going from the summary's remaining work.
 */

import { calculateContextTokens, estimateTokens, shouldCompact } from "../lib/pi-sdk/index.ts";
import { createModuleLogger } from "../lib/debug-log.ts";
import {
  COMPACTION_ASK_RATIO,
  COMPACTION_ASK_REPROMPT_DELTA_RATIO,
  COMPACTION_ASK_RESET_DROP_RATIO,
  COMPACTION_AUTO_EVENT,
  COMPACTION_FORCE_RATIO,
  COMPACTION_SUGGESTED_EVENT,
} from "../shared/compaction-thresholds.ts";
import {
  isDirectCompactionInProgress,
  runCachePreservingCompactionForSession,
} from "./session-compactor.ts";

const log = createModuleLogger("midrun-compaction");

/** Absolute floor, and the SDK's own default reserve. */
export const MIN_COMPACTION_RESERVE_TOKENS = 16_384;

/** Fraction of the context window kept free when it is larger than the floor implies. */
const COMPACTION_RESERVE_RATIO = 1 - COMPACTION_FORCE_RATIO;

const DYNAMIC_RESERVE_INSTALLED = Symbol("hanaDynamicCompactionReserve");
const MIDRUN_COMPACTION_INSTALLED = Symbol("hanaMidRunCompaction");

/**
 * Per-session ask-line state, keyed off the session object so it dies with the
 * session and never persists. `tokens` records the usage at the last ask so a
 * re-prompt only fires after usage grows by another 5% of the window.
 */
const COMPACTION_ASK_STATE = Symbol("hanaCompactionAskState");

/**
 * Custom message appended after a compaction that happened mid-task. Worded so
 * the model treats it as machine bookkeeping rather than a user instruction.
 */
export const MIDRUN_COMPACTION_NOTICE = `[System compaction notice — not a user message]
The conversation history above was compacted while you were actively working on the user's task. You are still mid-task. Continue the work described in the summary's "In Progress" and "Next Steps" sections without pausing to ask for confirmation, and do not redo work already listed as done. If any newer user message appears after this notice, it takes precedence over this notice.`;

/** Reserve tokens for a model window: the larger of the floor and (1 - force ratio) of the window. */
export function computeCompactionReserveTokens(contextWindow: any): number {
  const window = Number(contextWindow);
  if (!Number.isFinite(window) || window <= 0) return MIN_COMPACTION_RESERVE_TOKENS;
  return Math.max(MIN_COMPACTION_RESERVE_TOKENS, Math.ceil(window * COMPACTION_RESERVE_RATIO));
}

/**
 * Make this session's compaction reserve track its model window. The override
 * reads the model off the session on every call, so switching models moves the
 * trigger point with no further wiring.
 */
export function installDynamicCompactionReserve(session: any): void {
  const settingsManager = session?.settingsManager;
  if (!settingsManager) return;
  if (settingsManager[DYNAMIC_RESERVE_INSTALLED]) return;
  if (typeof settingsManager.getCompactionReserveTokens !== "function") {
    throw new Error(
      "installDynamicCompactionReserve: settings manager has no getCompactionReserveTokens; "
      + "the SDK compaction settings contract changed and the proportional reserve is no longer wired",
    );
  }
  settingsManager.getCompactionReserveTokens = () =>
    computeCompactionReserveTokens(session.model?.contextWindow);
  settingsManager[DYNAMIC_RESERVE_INSTALLED] = true;
}

/**
 * Check the compaction threshold at every turn boundary of an agentic run and
 * compact in place when it is crossed.
 *
 * @param session live AgentSession
 * @param deps.usageLedger ledger the compaction request bills to, or null
 * @param deps.buildUsageContext builds the usage attribution for this session
 * @param deps.runCompaction compaction entry point (injectable for tests)
 */
export function installMidRunCompaction(session: any, deps: {
  usageLedger?: any;
  buildUsageContext?: ((session: any) => any) | null;
  runCompaction?: (session: any, options: any) => Promise<any>;
} = {}): void {
  const agent = session?.agent;
  if (!agent) return;
  if (agent[MIDRUN_COMPACTION_INSTALLED]) return;

  const resolved = {
    usageLedger: deps.usageLedger ?? null,
    buildUsageContext: deps.buildUsageContext ?? null,
    runCompaction: deps.runCompaction ?? runCachePreservingCompactionForSession,
  };

  const previous = agent.prepareNextTurnWithContext
    ?? (agent.prepareNextTurn
      ? async (_turn: any, signal: any) => await agent.prepareNextTurn?.(signal)
      : undefined);

  agent.prepareNextTurnWithContext = async (turn: any, signal: any) => {
    const snapshot = await previous?.(turn, signal);
    const compacted = await maybeCompactMidRun(session, turn, signal, resolved);
    if (!compacted) return snapshot;
    const messages = Array.isArray(session.agent?.state?.messages)
      ? session.agent.state.messages.slice()
      : null;
    if (!messages) return snapshot;
    const base = snapshot?.context ?? turn?.context;
    return { ...snapshot, context: { ...base, messages } };
  };
  agent[MIDRUN_COMPACTION_INSTALLED] = true;
}

/**
 * @returns true when history was compacted and the caller must rebuild the
 *   turn context. Never throws: a failed mid-run compaction leaves the run
 *   alone and the SDK's post-run check picks it up.
 */
async function maybeCompactMidRun(session: any, turn: any, signal: any, deps: {
  usageLedger: any;
  buildUsageContext: ((session: any) => any) | null;
  runCompaction: (session: any, options: any) => Promise<any>;
}): Promise<boolean> {
  try {
    if (session.isCompacting === true || isDirectCompactionInProgress(session)) return false;

    const settings = session.settingsManager?.getCompactionSettings?.();
    if (!settings?.enabled) return false;

    const contextWindow = session.model?.contextWindow;
    if (!(contextWindow > 0)) return false;

    const message = turn?.message;
    if (!message || message.role !== "assistant") return false;
    if (message.stopReason === "error" || message.stopReason === "aborted") return false;
    if (!message.usage) return false;

    const usageTokens = calculateContextTokens(message.usage);
    if (!usageTokens) return false;

    // Usage reported before the latest compaction describes the pre-compaction
    // window and would retrigger compaction on the very next turn.
    const branch = session.sessionManager?.getBranch?.() || [];
    const compactionEntry = findLatestCompactionEntry(branch);
    if (compactionEntry && message.timestamp <= new Date(compactionEntry.timestamp).getTime()) {
      return false;
    }

    // Reported usage predates this turn's tool results, which are already in
    // context for the next request; estimate them so a burst of large tool
    // output is not invisible until the next assistant response.
    const toolResultTokens = Array.isArray(turn?.toolResults)
      ? turn.toolResults.reduce((sum: number, result: any) => sum + estimateTokens(result), 0)
      : 0;
    const contextTokens = usageTokens + toolResultTokens;
    const ratio = contextTokens / contextWindow;

    // FORCE line: compact unconditionally. The reserve derived from the same
    // ratio keeps the SDK's post-run check in agreement; the explicit ratio
    // check guards against a user-set reserve pushing the trigger later than
    // the promised 80%. Small windows (< 80K) fire earlier via shouldCompact
    // so the absolute 16384-token headroom is never violated.
    if (ratio >= COMPACTION_FORCE_RATIO || shouldCompact(contextTokens, contextWindow, settings)) {
      const percentBefore = Math.round(ratio * 100);
      await deps.runCompaction(session, {
        signal,
        emitLifecycle: true,
        lifecycleReason: "threshold",
        usageLedger: deps.usageLedger,
        usageContext: typeof deps.buildUsageContext === "function" ? deps.buildUsageContext(session) : null,
      });

      // A compaction just happened: the ask line starts over, and the user
      // gets told their context was compacted under them.
      delete session[COMPACTION_ASK_STATE];
      session?._emit?.({
        type: COMPACTION_AUTO_EVENT,
        percentBefore,
        forcePercent: Math.round(COMPACTION_FORCE_RATIO * 100),
      });

      session.sessionManager.appendCustomMessageEntry(
        "midrun-compaction-notice",
        MIDRUN_COMPACTION_NOTICE,
        false,
      );
      const context = session.sessionManager.buildSessionContext();
      session.agent.state.messages = context.messages;
      return true;
    }

    // ASK line: between 50% and the force line, hand the decision to the user
    // via the renderer. Never blocks the run; the renderer compacts (now or
    // after the run settles) only if the user accepts. Called unconditionally
    // so a post-compaction usage drop can reset the ask state even while the
    // ratio is still below the line.
    maybeEmitCompactionSuggestion(session, {
      contextTokens,
      contextWindow,
      ratio,
    });
    return false;
  } catch (err: any) {
    if (err?.name === "AbortError" || signal?.aborted) {
      log.log("mid-run compaction aborted");
      return false;
    }
    log.warn(`mid-run compaction failed, continuing the run: ${err?.message || String(err)}`);
    return false;
  }
}

function findLatestCompactionEntry(branch: any[]) {
  for (let i = branch.length - 1; i >= 0; i--) {
    if (branch[i]?.type === "compaction") return branch[i];
  }
  return null;
}

/**
 * Emit `compaction_suggested` to the renderer at most once per growth segment.
 *
 * State machine on session[COMPACTION_ASK_STATE]:
 *   - usage dropped >= 20% of window since last ask → a compaction happened;
 *     reset the state and stay silent until the line is crossed again
 *   - no state, ratio >= ask line                  → ask now, remember usage
 *   - ratio below the ask line                     → stay silent
 *   - usage grew < 5% of window since last ask     → stay silent
 *   - otherwise                                    → ask again, re-record usage
 */
function maybeEmitCompactionSuggestion(session: any, info: {
  contextTokens: number;
  contextWindow: number;
  ratio: number;
}): void {
  try {
    const { contextTokens, contextWindow, ratio } = info;
    const state = session[COMPACTION_ASK_STATE] as { tokens: number } | undefined;
    if (state
      && state.tokens - contextTokens >= contextWindow * COMPACTION_ASK_RESET_DROP_RATIO) {
      // A compaction happened under us: forget the old anchor. The next
      // crossing of the ask line prompts afresh.
      delete session[COMPACTION_ASK_STATE];
      return;
    }
    if (ratio < COMPACTION_ASK_RATIO) return;
    if (state
      && contextTokens - state.tokens < contextWindow * COMPACTION_ASK_REPROMPT_DELTA_RATIO) {
      return;
    }
    const sessionPath = session.sessionManager?.getSessionFile?.();
    if (!sessionPath) return;
    session[COMPACTION_ASK_STATE] = { tokens: contextTokens };
    session?._emit?.({
      type: COMPACTION_SUGGESTED_EVENT,
      sessionPath,
      percent: Math.round(ratio * 100),
      askPercent: Math.round(COMPACTION_ASK_RATIO * 100),
      forcePercent: Math.round(COMPACTION_FORCE_RATIO * 100),
    });
  } catch (err: any) {
    log.warn(`compaction suggestion emit failed: ${err?.message || String(err)}`);
  }
}
