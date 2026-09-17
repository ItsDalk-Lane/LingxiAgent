/**
 * checkpoint-tool.ts — 会话具名存档点工具（阶段二·8）。
 *
 * create：侧车记录 {target=当时最新用户输入, turnInputEntryId, 快照 commit}
 * （snapshot=true 时先拍影子仓库快照，label 带 checkpoint 名）；list / drop
 * 管存档点。名字唯一：latest 保留名重复 create 覆盖，其余冲突拒绝。
 * 权限：create/drop=write（计划模式不给留标记），list=read。
 */
import { Type, StringEnum } from "../pi-sdk/index.ts";
import {
  listSessionCheckpoints,
  upsertSessionCheckpoint,
  dropSessionCheckpoint,
  SESSION_CHECKPOINT_RESERVED_NAME,
} from "../../core/session-checkpoints.ts";

export interface CheckpointToolDeps {
  getSessionPath: () => string | null;
  /** 拍影子仓库快照：({sessionPath, label}) → { commit, degraded } | null */
  captureSnapshot: (args: { sessionPath: string; label: string }) => Promise<{ commit: string | null; degraded: boolean } | null>;
  /** 当前 branch 上最新用户输入 entry（{id, turnInputEntryId}）*/
  getLatestUserEntry: (sessionPath: string) => { id: string; turnInputEntryId: string | null } | null;
  /** 当前 branch 的内存消息计数（存档点元信息）。 */
  getMessageCount: (sessionPath: string) => number;
}

export function createCheckpointTool(deps: CheckpointToolDeps) {
  return {
    name: "checkpoint",
    description: "Create named session checkpoints so a later rewind tool call can restore the conversation (and optionally workspace files) to this point. Create one before risky refactors or long experiments. Checkpoints are session-local sidecar records plus an optional workspace snapshot; 'latest' is a reserved rolling slot (re-creating overwrites it), other names are unique — creating an existing name fails.",
    parameters: Type.Object({
      action: StringEnum(["create", "list", "drop"], { description: "create: record a checkpoint at the current conversation point (default). list: list checkpoints. drop: remove one by name" }),
      name: Type.String({ description: "Checkpoint name; default 'latest' (rolling slot)" }),
      snapshot: Type.Boolean({ description: "Also capture a workspace snapshot so rewind can restore files (default true)" }),
    }),
    sessionPermission: {
      resolveInvocation: (input: any = {}) => {
        if (input?.action === "list") {
          return { action: "list", kind: "read", capability: "checkpoint.list" };
        }
        return { action: input?.action === "drop" ? "drop" : "create", kind: "write", capability: "checkpoint.write" };
      },
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      const action = params?.action === "list" || params?.action === "drop" ? params.action : "create";
      const sessionPath = deps.getSessionPath?.() || null;
      if (!sessionPath) {
        return { isError: true, content: [{ type: "text", text: "no active session — checkpoints are session-scoped" }] };
      }

      if (action === "list") {
        const records = listSessionCheckpoints(sessionPath);
        if (!records.length) {
          return { content: [{ type: "text", text: "no checkpoints yet" }], details: { count: 0 } };
        }
        const lines = records.map((r) => (
          `- ${r.name} (created ${new Date(r.createdAt).toISOString()}, messages=${r.messageCount}, `
          + `${r.snapshotCommit ? "workspace snapshot ✓" : r.snapshotDegraded ? "snapshot degraded" : "no file snapshot"})`
        ));
        return {
          content: [{ type: "text", text: ["checkpoints:", ...lines].join("\n") }],
          details: { count: records.length, checkpoints: records.map((r) => ({ name: r.name, createdAt: r.createdAt })) },
        };
      }

      if (action === "drop") {
        const name = typeof params?.name === "string" && params.name.trim() ? params.name.trim() : "";
        if (!name) {
          return { content: [{ type: "text", text: "name is required for drop" }] };
        }
        const dropped = dropSessionCheckpoint(sessionPath, name);
        return {
          content: [{ type: "text", text: dropped ? `checkpoint "${name}" dropped` : `checkpoint "${name}" not found` }],
          details: { dropped },
        };
      }

      // ── create ──
      const name = typeof params?.name === "string" && params.name.trim()
        ? params.name.trim()
        : SESSION_CHECKPOINT_RESERVED_NAME;
      const latest = deps.getLatestUserEntry?.(sessionPath) || null;
      if (!latest) {
        return { isError: true, content: [{ type: "text", text: "no user turn on the active branch yet — nothing to anchor a checkpoint to" }] };
      }
      let snapshot: { commit: string | null; degraded: boolean } | null = null;
      if (params?.snapshot !== false) {
        try {
          snapshot = await deps.captureSnapshot?.({ sessionPath, label: `checkpoint:${name}` }) || null;
        } catch {
          snapshot = { commit: null, degraded: true };
        }
      }
      try {
        const record = upsertSessionCheckpoint(sessionPath, {
          name,
          target: { role: "user", entryId: latest.id },
          turnInputEntryId: latest.turnInputEntryId ?? latest.id,
          snapshotCommit: snapshot?.commit ?? null,
          snapshotDegraded: snapshot?.degraded === true,
          messageCount: deps.getMessageCount?.(sessionPath) ?? 0,
        });
        return {
          content: [{
            type: "text",
            text: [
              `checkpoint "${record.name}" created (${record.messageCount} messages retained to this point)`,
              snapshot?.commit
                ? "workspace snapshot captured — rewind with restoreFiles=true can bring files back to this point"
                : snapshot?.degraded
                  ? "workspace snapshot degraded (files not restorable to this point; conversation rewind still works)"
                  : "no workspace snapshot (snapshot=false)",
              `rewind here later with the rewind tool: { checkpoint: "${record.name}" }`,
            ].join("\n"),
          }],
          details: { checkpoint: record.name, createdAt: record.createdAt, snapshotCommit: record.snapshotCommit },
        };
      } catch (err: any) {
        return { isError: true, content: [{ type: "text", text: err?.message || String(err) }] };
      }
    },
  };
}
