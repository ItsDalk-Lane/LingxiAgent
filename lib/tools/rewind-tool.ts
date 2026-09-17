/**
 * rewind-tool.ts — 回到具名存档点（阶段二·8），破坏性操作默认确认卡点头。
 *
 * 流程：读存档点 → （restoreFiles=true 时）预览将还原的文件清单 →
 * ConfirmStore 阻塞确认（卡里附清单）→ 用户 confirmed 才调
 * rewindToCheckpoint 事务（截断对话分支 + 可选影子仓库文件还原 +
 * 派生记忆失效 + 取消被丢分支后台任务）。拒绝/超时：如实返回「未回滚」，
 * 绝不硬来。快照不可用时 restoreFiles 降级提示（对话回滚仍可做）。
 * 权限：kind write（只读档拦截——回滚本身改写会话历史）。
 */
import { Type } from "../pi-sdk/index.ts";
import { t } from "../i18n.ts";

const CONFIRM_TIMEOUT_MS = 5 * 60_000;
const PREVIEW_FILE_CAP = 50;

export interface RewindToolDeps {
  getSessionPath: () => string | null;
  getConfirmStore: () => any;
  emitEvent: (event: any, sessionPath: string) => void;
  /** core/session-turn-actions.ts rewindToCheckpoint 的绑定。 */
  rewindToCheckpoint: (opts: Record<string, any>) => Promise<any>;
  /** 影子仓库预览：({sessionPath, checkpointName, createdAtHint}) → {available, degraded, files[], fileCount} | null */
  previewRestoreFiles: (args: { sessionPath: string; checkpointName: string; createdAtHint: number | null }) => Promise<any>;
}

export function createRewindTool(deps: RewindToolDeps) {
  return {
    name: "rewind",
    description: "Rewind the conversation to a named checkpoint (default 'latest'), dropping everything after it. Destructive: by default a confirmation card is shown to the user listing what will happen (and which files would be restored) — the rewind only runs after the user confirms. restoreFiles=true additionally restores workspace files from the checkpoint's snapshot (per-file report; file restore needs the rollback preference enabled). After rewind the dropped background tasks are cancelled and derived memory is invalidated automatically. Always create a checkpoint first with the checkpoint tool.",
    parameters: Type.Object({
      checkpoint: Type.String({ description: "Checkpoint name to rewind to (default 'latest')" }),
      restoreFiles: Type.Boolean({ description: "Also restore workspace files captured at checkpoint time (default false)" }),
      /** 模型不得自行置 true 绕过用户：仅当上一轮已确认但执行失败重试时由系统回填。 */
      risk_accepted: Type.Boolean({ description: "Reserved; leave unset" }),
    }),
    sessionPermission: {
      resolveInvocation: (_input: any = {}) => ({
        action: "apply",
        kind: "write",
        capability: "rewind.apply",
      }),
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      const sessionPath = deps.getSessionPath?.() || null;
      if (!sessionPath) {
        return { isError: true, content: [{ type: "text", text: "no active session — rewind is session-scoped" }] };
      }
      const checkpointName = typeof params?.checkpoint === "string" && params.checkpoint.trim()
        ? params.checkpoint.trim()
        : "latest";
      const restoreFiles = params?.restoreFiles === true;

      // ── 文件还原预览（尽力；快照不可用 → 提示降级，对话回滚不受影响）──
      let filePreview: any = null;
      if (restoreFiles) {
        try {
          filePreview = await deps.previewRestoreFiles({ sessionPath, checkpointName, createdAtHint: null });
        } catch {
          filePreview = null;
        }
      }
      const previewFiles: string[] = Array.isArray(filePreview?.files)
        ? filePreview.files.slice(0, PREVIEW_FILE_CAP).map((f: any) => `${f.status === "A" ? "+ new" : f.status === "D" ? "- deleted" : "~ changed"} ${f.path}`)
        : [];

      // ── 确认卡：破坏性操作默认要用户点头 ──
      const confirmStore = deps.getConfirmStore?.() || null;
      if (!confirmStore) {
        return { isError: true, content: [{ type: "text", text: t("error.rewindUnavailable") }] };
      }
      const bodyLines = [
        t("rewind.confirm.body", { name: checkpointName }),
        ...(restoreFiles
          ? filePreview?.degraded || !filePreview?.available
            ? [t("rewind.confirm.snapshotUnavailable")]
            : previewFiles.length
              ? [t("rewind.confirm.filesTitle", { count: filePreview.fileCount }), ...previewFiles]
              : [t("rewind.confirm.noFileChanges")]
          : []),
      ];
      const { confirmId, promise } = confirmStore.create(
        "rewind",
        { checkpoint: checkpointName, restoreFiles, previewFiles },
        sessionPath,
        CONFIRM_TIMEOUT_MS,
      );
      deps.emitEvent?.({
        type: "session_confirmation",
        request: {
          type: "session_confirmation",
          confirmId,
          kind: "rewind",
          surface: "input",
          status: "pending",
          title: t("rewind.confirm.title"),
          body: bodyLines.join("\n"),
          subject: { label: t("rewind.confirm.subject"), detail: checkpointName },
          severity: "warning",
          actions: { confirmLabel: t("rewind.confirm.accept"), rejectLabel: t("rewind.confirm.reject") },
          payload: { checkpoint: checkpointName, restoreFiles, files: previewFiles },
        },
      }, sessionPath);

      const decision = await promise;
      if (decision?.action !== "confirmed") {
        const reason = decision?.action === "timeout"
          ? t("rewind.result.timeout")
          : decision?.action === "aborted"
            ? t("rewind.result.aborted")
            : t("rewind.result.rejected");
        return {
          content: [{ type: "text", text: reason }],
          details: { rewind: false, action: decision?.action || "rejected", checkpoint: checkpointName },
        };
      }

      // ── 用户已点头：执行事务 ──
      try {
        const result = await deps.rewindToCheckpoint({ sessionPath, checkpointName, restoreFiles });
        const report = result?.fileRollbackReport;
        const lines = [
          t("rewind.result.done", { name: checkpointName, count: result?.discardedEntries ?? 0 }),
        ];
        if (restoreFiles && report) {
          if (report.ok) {
            lines.push(t("rewind.result.filesOk", { count: (report.files || []).length }));
          } else {
            lines.push(t("rewind.result.filesDegraded", { reason: report.reason || "unavailable" }));
          }
        }
        return {
          content: [{ type: "text", text: lines.join("\n") }],
          details: { rewind: true, checkpoint: checkpointName, discardedEntries: result?.discardedEntries, fileRollbackReport: report },
        };
      } catch (err: any) {
        return {
          isError: true,
          content: [{ type: "text", text: `${t("rewind.result.failed")}: ${err?.message || String(err)}` }],
          details: { rewind: false, checkpoint: checkpointName },
        };
      }
    },
  };
}
