/**
 * model-observability-trace-store.ts — ModelCallEvent → SQLite 投影（Phase 7）。
 *
 * 消费 Phase 1 契约的 lifecycle 事件，投影为 durable rows：
 *
 *   logical_call_start          → INSERT/UPDATE model_calls + UPSERT traces
 *   attempt_start               → INSERT/UPSERT model_attempts
 *   provider_request_prepared   → UPDATE attempt.request_prepared_at
 *   provider_response_received  → UPDATE attempt response/requstId/httpStatus
 *   semantic_response_completed → UPDATE call.semantic_completed_at
 *   attempt_error               → UPDATE attempt error 事实
 *   logical_call_error/aborted  → 记录安全终态/error 事实（end 才落 terminal）
 *   logical_call_end            → ended_at + terminal_status
 *
 * 事实纪律（任务书 §二十一～二十五）：
 *   - 事件可以缺失（MC-03 无 provider_request_prepared 等）→ 对应列保持 NULL，
 *     NULL 不是 corruption。
 *   - 不从 payload 反推 observer 事件；不虚构 started time。
 *   - payload record 先到而 call row 尚未出现 → callShellFromIdentity 建
 *     partial shell（started_at NULL），后续事件补齐。
 *   - Store 是持久化事实投影，不是 Model Call 真相唯一来源（§二十五）。
 *
 * 写入方法必须在 coordinator 的 transaction 内调用（本模块不自行开事务）。
 */

import type { ModelCallEvent } from "./model-call-observer.ts";

const STRING_MAX = 512;
const JSON_MAX_CHARS = 262_144;

function textOrNull(value: unknown, max: number = STRING_MAX): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  return trimmed.length > max ? trimmed.slice(0, max) : trimmed;
}

function intOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function usageCorrelationStateOrNull(value: unknown): "not_correlated" | null {
  return value === "not_correlated" ? value : null;
}

function jsonOrNull(value: unknown): string | null {
  if (value === null || value === undefined) return null;
  try {
    const text = JSON.stringify(value);
    return text.length > JSON_MAX_CHARS ? null : text;
  } catch {
    return null;
  }
}

export type ModelObservabilityCallRow = Record<string, unknown> & {
  call_id: string;
  trace_id: string | null;
  parent_call_id: string | null;
};

export type ModelObservabilityTraceRow = Record<string, unknown> & {
  trace_id: string;
};

export type ModelObservabilityAttemptRow = Record<string, unknown> & {
  attempt_id: string;
  call_id: string;
};

/** 从 attribution 里提取的高频查询列（§十三：稳定维度独立列 + index）。 */
function attributionColumns(attribution: Record<string, unknown> | null | undefined) {
  const a = attribution && typeof attribution === "object" ? attribution : {};
  return {
    attribution_kind: textOrNull(a.kind, 128),
    session_id: textOrNull(a.sessionId, 256),
    session_path: textOrNull(a.sessionPath, 1024),
    conversation_id: textOrNull(a.conversationId, 256),
    conversation_type: textOrNull(a.conversationType, 64),
    agent_id: textOrNull(a.agentId, 256),
    child_agent_id: textOrNull(a.childAgentId, 256),
    child_session_id: textOrNull(a.childSessionId, 256),
    child_session_path: textOrNull(a.childSessionPath, 1024),
    task_id: textOrNull(a.taskId, 256),
  };
}

/** details 中已知的结构 metadata 列（经 recorder 安全门的 shape 信息）。 */
function detailColumns(details: Record<string, unknown> | null | undefined) {
  const d = details && typeof details === "object" ? details : {};
  const categories = Array.isArray(d.inputCategories)
    ? d.inputCategories.filter((item): item is string => typeof item === "string").slice(0, 32)
    : null;
  return {
    call_purpose: textOrNull(d.callPurpose, 128),
    input_shape: textOrNull(d.inputShape, 64),
    provenance_precision: textOrNull(d.provenancePrecision, 32),
    provenance_section_count: intOrNull(d.inputSectionCount),
    provenance_categories_json: categories ? JSON.stringify(categories) : null,
    provenance_opaque_count: intOrNull(d.opaqueSectionCount),
  };
}

export function createModelObservabilityTraceStore({ db, now = () => new Date().toISOString() }: {
  db: any;
  now?: () => string;
}) {
  const stmts = {
    upsertTrace: db.prepare(`
      INSERT INTO traces (trace_id, origin, first_seen_at, last_seen_at, call_count, created_at, updated_at)
      VALUES (@trace_id, @origin, @ts, @ts, @call_count, @ts, @ts)
      ON CONFLICT(trace_id) DO UPDATE SET
        last_seen_at = MAX(last_seen_at, excluded.last_seen_at),
        first_seen_at = MIN(first_seen_at, excluded.first_seen_at),
        origin = COALESCE(traces.origin, excluded.origin),
        call_count = traces.call_count + excluded.call_count,
        updated_at = excluded.updated_at
    `),
    touchTrace: db.prepare(`
      INSERT INTO traces (trace_id, origin, first_seen_at, last_seen_at, call_count, created_at, updated_at)
      VALUES (@trace_id, NULL, @ts, @ts, 0, @ts, @ts)
      ON CONFLICT(trace_id) DO UPDATE SET
        last_seen_at = MAX(last_seen_at, excluded.last_seen_at),
        updated_at = excluded.updated_at
    `),
    insertCall: db.prepare(`
      INSERT INTO model_calls (
        call_id, trace_id, parent_call_id,
        provider, model_id, api,
        subsystem, operation, surface, trigger,
        attribution_kind, session_id, session_path, conversation_id, conversation_type,
        agent_id, child_agent_id, child_session_id, child_session_path, task_id,
        call_purpose,
        started_at,
        input_shape, provenance_precision, provenance_section_count,
        provenance_categories_json, provenance_opaque_count,
        attribution_json, source_json, safe_details_json,
        usage_correlation_state,
        persistence_completeness
      ) VALUES (
        @call_id, @trace_id, @parent_call_id,
        @provider, @model_id, @api,
        @subsystem, @operation, @surface, @trigger,
        @attribution_kind, @session_id, @session_path, @conversation_id, @conversation_type,
        @agent_id, @child_agent_id, @child_session_id, @child_session_path, @task_id,
        @call_purpose,
        @started_at,
        @input_shape, @provenance_precision, @provenance_section_count,
        @provenance_categories_json, @provenance_opaque_count,
        @attribution_json, @source_json, @safe_details_json,
        @usage_correlation_state,
        'partial'
      )
      ON CONFLICT(call_id) DO UPDATE SET
        trace_id = COALESCE(model_calls.trace_id, excluded.trace_id),
        parent_call_id = COALESCE(model_calls.parent_call_id, excluded.parent_call_id),
        provider = COALESCE(model_calls.provider, excluded.provider),
        model_id = COALESCE(model_calls.model_id, excluded.model_id),
        api = COALESCE(model_calls.api, excluded.api),
        subsystem = COALESCE(model_calls.subsystem, excluded.subsystem),
        operation = COALESCE(model_calls.operation, excluded.operation),
        surface = COALESCE(model_calls.surface, excluded.surface),
        trigger = COALESCE(model_calls.trigger, excluded.trigger),
        attribution_kind = COALESCE(model_calls.attribution_kind, excluded.attribution_kind),
        session_id = COALESCE(model_calls.session_id, excluded.session_id),
        session_path = COALESCE(model_calls.session_path, excluded.session_path),
        conversation_id = COALESCE(model_calls.conversation_id, excluded.conversation_id),
        conversation_type = COALESCE(model_calls.conversation_type, excluded.conversation_type),
        agent_id = COALESCE(model_calls.agent_id, excluded.agent_id),
        child_agent_id = COALESCE(model_calls.child_agent_id, excluded.child_agent_id),
        child_session_id = COALESCE(model_calls.child_session_id, excluded.child_session_id),
        child_session_path = COALESCE(model_calls.child_session_path, excluded.child_session_path),
        task_id = COALESCE(model_calls.task_id, excluded.task_id),
        call_purpose = COALESCE(model_calls.call_purpose, excluded.call_purpose),
        started_at = COALESCE(model_calls.started_at, excluded.started_at),
        input_shape = COALESCE(model_calls.input_shape, excluded.input_shape),
        provenance_precision = COALESCE(model_calls.provenance_precision, excluded.provenance_precision),
        provenance_section_count = COALESCE(model_calls.provenance_section_count, excluded.provenance_section_count),
        provenance_categories_json = COALESCE(model_calls.provenance_categories_json, excluded.provenance_categories_json),
        provenance_opaque_count = COALESCE(model_calls.provenance_opaque_count, excluded.provenance_opaque_count),
        attribution_json = COALESCE(model_calls.attribution_json, excluded.attribution_json),
        source_json = COALESCE(model_calls.source_json, excluded.source_json),
        safe_details_json = COALESCE(model_calls.safe_details_json, excluded.safe_details_json),
        usage_correlation_state = COALESCE(
          model_calls.usage_correlation_state,
          excluded.usage_correlation_state
        ),
        persistence_completeness = CASE
          WHEN model_calls.persistence_completeness = 'complete' THEN 'complete'
          ELSE 'partial' END
    `),
    insertAttempt: db.prepare(`
      INSERT INTO model_attempts (attempt_id, call_id, started_at, attempt_visibility, provider_wire_visibility)
      VALUES (@attempt_id, @call_id, @started_at, @attempt_visibility, @provider_wire_visibility)
      ON CONFLICT(attempt_id) DO UPDATE SET
        call_id = COALESCE(NULLIF(model_attempts.call_id, ''), excluded.call_id),
        started_at = COALESCE(model_attempts.started_at, excluded.started_at),
        attempt_visibility = COALESCE(model_attempts.attempt_visibility, excluded.attempt_visibility),
        provider_wire_visibility = COALESCE(model_attempts.provider_wire_visibility, excluded.provider_wire_visibility)
    `),
    updateAttemptRequestPrepared: db.prepare(
      `UPDATE model_attempts SET
        request_prepared_at = COALESCE(request_prepared_at, @ts),
        safe_details_json = COALESCE(safe_details_json, @safe_details_json)
      WHERE attempt_id = @attempt_id`,
    ),
    updateAttemptResponse: db.prepare(`
      UPDATE model_attempts SET
        response_received_at = COALESCE(response_received_at, @ts),
        http_status = COALESCE(http_status, @http_status),
        provider_request_id = COALESCE(provider_request_id, @provider_request_id)
      WHERE attempt_id = @attempt_id
    `),
    updateAttemptError: db.prepare(`
      UPDATE model_attempts SET
        error_at = COALESCE(error_at, @ts),
        error_name = COALESCE(error_name, @error_name),
        error_code = COALESCE(error_code, @error_code)
      WHERE attempt_id = @attempt_id
    `),
    updateCallSemanticCompleted: db.prepare(
      `UPDATE model_calls SET semantic_completed_at = COALESCE(semantic_completed_at, @ts) WHERE call_id = @call_id`,
    ),
    updateCallError: db.prepare(`
      UPDATE model_calls SET
        error_name = COALESCE(error_name, @error_name),
        error_code = COALESCE(error_code, @error_code)
      WHERE call_id = @call_id
    `),
    updateCallEnd: db.prepare(`
      UPDATE model_calls SET
        ended_at = COALESCE(ended_at, @ts),
        terminal_status = COALESCE(terminal_status, @terminal_status),
        persistence_completeness = CASE
          WHEN started_at IS NOT NULL THEN 'complete'
          ELSE persistence_completeness END
      WHERE call_id = @call_id
    `),
  };

  function ensureCallShell(event: ModelCallEvent): void {
    const model = (event.model ?? {}) as Record<string, unknown>;
    const source = (event.source ?? {}) as Record<string, unknown>;
    stmts.insertCall.run({
      call_id: event.callId,
      trace_id: textOrNull(event.traceId, 256),
      parent_call_id: textOrNull(event.parentCallId, 256),
      provider: textOrNull(model.provider, 128),
      model_id: textOrNull(model.modelId, 256),
      api: textOrNull(model.api, 128),
      subsystem: textOrNull(source.subsystem, 128),
      operation: textOrNull(source.operation, 128),
      surface: textOrNull(source.surface, 128),
      trigger: textOrNull(source.trigger, 128),
      ...attributionColumns(event.attribution),
      ...detailColumns(event.details),
      started_at: null,
      attribution_json: jsonOrNull(event.attribution),
      source_json: jsonOrNull(event.source),
      safe_details_json: jsonOrNull(event.details),
      usage_correlation_state: usageCorrelationStateOrNull(event.usageCorrelation),
    });
    if (event.traceId) {
      stmts.touchTrace.run({ trace_id: event.traceId, ts: event.timestamp || now() });
    }
  }

  function applyEvent(event: ModelCallEvent): void {
    if (!event || typeof event.callId !== "string" || !event.callId.trim()) return;
    const ts = textOrNull(event.timestamp, 64) ?? now();
    const details = event.details && typeof event.details === "object" ? event.details : {};

    switch (event.eventType) {
      case "logical_call_start": {
        const model = (event.model ?? {}) as Record<string, unknown>;
        const source = (event.source ?? {}) as Record<string, unknown>;
        stmts.insertCall.run({
          call_id: event.callId,
          trace_id: textOrNull(event.traceId, 256),
          parent_call_id: textOrNull(event.parentCallId, 256),
          provider: textOrNull(model.provider, 128),
          model_id: textOrNull(model.modelId, 256),
          api: textOrNull(model.api, 128),
          subsystem: textOrNull(source.subsystem, 128),
          operation: textOrNull(source.operation, 128),
          surface: textOrNull(source.surface, 128),
          trigger: textOrNull(source.trigger, 128),
          ...attributionColumns(event.attribution),
          ...detailColumns(details),
          started_at: ts,
          attribution_json: jsonOrNull(event.attribution),
          source_json: jsonOrNull(event.source),
          safe_details_json: jsonOrNull(details),
          usage_correlation_state: usageCorrelationStateOrNull(event.usageCorrelation),
        });
        if (event.traceId) {
          stmts.upsertTrace.run({
            trace_id: event.traceId,
            origin: textOrNull(details.traceOrigin, 64),
            ts,
            call_count: 1,
          });
        }
        return;
      }
      case "attempt_start": {
        const attemptId = textOrNull(event.attemptId, 256);
        if (!attemptId) return;
        // 队列溢出可能先丢 logical_call_start：attempt 仍保真（shell call）。
        ensureCallShell(event);
        stmts.insertAttempt.run({
          attempt_id: attemptId,
          call_id: event.callId,
          started_at: ts,
          attempt_visibility: textOrNull(details.attemptVisibility, 64),
          provider_wire_visibility: textOrNull(details.providerWireVisibility, 64),
        });
        return;
      }
      case "provider_request_prepared": {
        const attemptId = textOrNull(event.attemptId, 256);
        if (!attemptId) return;
        stmts.insertAttempt.run({
          attempt_id: attemptId,
          call_id: event.callId,
          started_at: null,
          attempt_visibility: null,
          provider_wire_visibility: null,
        });
        // Output Budget Fact 等 prepared 结构 metadata 持久化进
        // safe_details_json（借鉴 deepseek-harness materialized request header）。
        stmts.updateAttemptRequestPrepared.run({
          attempt_id: attemptId,
          ts,
          safe_details_json: jsonOrNull(details),
        });
        return;
      }
      case "provider_response_received": {
        const attemptId = textOrNull(event.attemptId, 256);
        if (!attemptId) return;
        stmts.insertAttempt.run({
          attempt_id: attemptId,
          call_id: event.callId,
          started_at: null,
          attempt_visibility: null,
          provider_wire_visibility: null,
        });
        stmts.updateAttemptResponse.run({
          attempt_id: attemptId,
          ts,
          http_status: intOrNull(details.httpStatus),
          provider_request_id: textOrNull(event.providerRequestId, 128),
        });
        return;
      }
      case "semantic_response_completed": {
        stmts.updateCallSemanticCompleted.run({ call_id: event.callId, ts });
        return;
      }
      case "attempt_error": {
        const attemptId = textOrNull(event.attemptId, 256);
        if (!attemptId) return;
        stmts.insertAttempt.run({
          attempt_id: attemptId,
          call_id: event.callId,
          started_at: null,
          attempt_visibility: null,
          provider_wire_visibility: null,
        });
        const error = (event.error ?? {}) as Record<string, unknown>;
        stmts.updateAttemptError.run({
          attempt_id: attemptId,
          ts,
          error_name: textOrNull(error.name, 128),
          error_code: textOrNull(error.code, 128),
        });
        return;
      }
      case "logical_call_error":
      case "logical_call_aborted": {
        const error = (event.error ?? {}) as Record<string, unknown>;
        stmts.updateCallError.run({
          call_id: event.callId,
          error_name: textOrNull(error.name, 128),
          error_code: textOrNull(error.code, 128),
        });
        return;
      }
      case "logical_call_end": {
        const status = event.status === "ok" || event.status === "error" || event.status === "aborted"
          ? event.status
          : null;
        stmts.updateCallEnd.run({ call_id: event.callId, ts, terminal_status: status });
        return;
      }
      default:
        return;
    }
  }

  return {
    applyEvent,

    /** payload 先到时的 partial call shell（§二十三）：不虚构 started time。 */
    callShellFromIdentity(identity: {
      callId: string;
      traceId?: string | null;
      parentCallId?: string | null;
      model?: Record<string, unknown> | null;
      source?: Record<string, unknown> | null;
      attribution?: Record<string, unknown> | null;
    }): void {
      const model = (identity.model ?? {}) as Record<string, unknown>;
      const source = (identity.source ?? {}) as Record<string, unknown>;
      stmts.insertCall.run({
        call_id: identity.callId,
        trace_id: textOrNull(identity.traceId, 256),
        parent_call_id: textOrNull(identity.parentCallId, 256),
        provider: textOrNull(model.provider, 128),
        model_id: textOrNull(model.modelId, 256),
        api: textOrNull(model.api, 128),
        subsystem: textOrNull(source.subsystem, 128),
        operation: textOrNull(source.operation, 128),
        surface: textOrNull(source.surface, 128),
        trigger: textOrNull(source.trigger, 128),
        ...attributionColumns(identity.attribution),
        call_purpose: null,
        started_at: null,
        input_shape: null,
        provenance_precision: null,
        provenance_section_count: null,
        provenance_categories_json: null,
        provenance_opaque_count: null,
        attribution_json: jsonOrNull(identity.attribution),
        source_json: jsonOrNull(identity.source),
        safe_details_json: null,
        usage_correlation_state: null,
      });
      if (identity.traceId) {
        stmts.touchTrace.run({ trace_id: identity.traceId, ts: now() });
      }
    },

    /**
     * Startup Reconciliation（§四十六/四十七）：崩溃遗留的未完成 call 只标记
     * interrupted_by_restart（persistence inference，非 terminal fact），
     * terminal_status 保持 NULL——进程中断不等于 Provider error。
     */
    reconcileAfterRestart(): number {
      const info = db.prepare(
        `UPDATE model_calls SET interrupted_by_restart = 1
         WHERE terminal_status IS NULL AND ended_at IS NULL AND interrupted_by_restart = 0`,
      ).run();
      return Number(info.changes || 0);
    },

    /* ── 内部 exact-identity 读原语（§一百一十八；无 filter/pagination）── */

    getCall(callId: string): ModelObservabilityCallRow | null {
      return db.prepare(`SELECT * FROM model_calls WHERE call_id = ?`).get(callId) ?? null;
    },

    getTrace(traceId: string): ModelObservabilityTraceRow | null {
      return db.prepare(`SELECT * FROM traces WHERE trace_id = ?`).get(traceId) ?? null;
    },

    /**
     * 会话级轨迹复用查找（产品口径 2026-09-05）：该会话最近一次有任务上下文
     * 的轨迹（origin 非空——user_turn/bridge/slash 等任务根；origin 为空的
     * singleton 辅助调用不是对话轨迹，不得作为复用目标）。同会话后续 turn
     * 复用它，调用在原记录上累加。未启用/无历史/查不到 → null（铸新根）。
     * 实测注记（2026-09-05）：桌面 turn 历史上经 pi ingress 落成 origin=
     * unknown，因此这里不得按 origin='user_turn' 过滤。
     */
    findReusableSessionTraceId(sessionId: string | null | undefined): string | null {
      if (typeof sessionId !== "string" || !sessionId.trim()) return null;
      const row = db.prepare(
        `SELECT c.trace_id AS trace_id
         FROM model_calls c JOIN traces t ON t.trace_id = c.trace_id
         WHERE c.session_id = ? AND t.origin IS NOT NULL
         ORDER BY c.started_at DESC, c.call_id DESC
         LIMIT 1`,
      ).get(sessionId.trim()) as { trace_id?: unknown } | undefined;
      return typeof row?.trace_id === "string" && row.trace_id ? row.trace_id : null;
    },

    getAttempts(callId: string): ModelObservabilityAttemptRow[] {
      return db.prepare(`SELECT * FROM model_attempts WHERE call_id = ? ORDER BY started_at, rowid`).all(callId);
    },

    markPayloadAvailability(callIds: string[], availability: "expired" | "dropped" | "not_captured"): void {
      if (callIds.length === 0) return;
      const update = db.prepare(
        `UPDATE model_calls SET payload_availability = CASE
           WHEN @availability = 'dropped' THEN 'dropped'
           WHEN @availability = 'expired' AND payload_availability IS NOT 'dropped' THEN 'expired'
           WHEN @availability = 'not_captured' AND payload_availability IS NULL THEN 'not_captured'
           ELSE payload_availability
         END
         WHERE call_id = @call_id`,
      );
      for (const callId of callIds) {
        if (typeof callId === "string" && callId) update.run({ availability, call_id: callId });
      }
    },
  };
}

export type ModelObservabilityTraceStore = ReturnType<typeof createModelObservabilityTraceStore>;
