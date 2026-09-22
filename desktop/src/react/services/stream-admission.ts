/** 流身份只用于传输隔离；Run、模型轮次和会话 trace 不互相充当身份。 */
export interface StreamIdentityEvent {
  type?: unknown;
  streamId?: unknown;
  runId?: unknown;
  isStreaming?: unknown;
}

const MAX_RETIRED_IDENTITIES = 256;
const identity = (value: unknown): string | null => typeof value === 'string' && value.trim() ? value.trim() : null;

/** 被真实 WS 分发和缓冲入口共用；饱和后要求恢复重建，不遗忘旧身份再误接纳。 */
export class StreamAdmission {
  streamId: string | null = null;
  runId: string | null = null;
  recoveryRequired = false;
  recoveryRequested = false;
  private retiredStreams = new Set<string>();
  private retiredRuns = new Set<string>();
  private authoritativeSwitchOnly = false;

  /** 仅恢复服务收到当前服务器快照后调用；保留旧流屏障，饱和后每次换流都要求快照。 */
  restore(streamId: string | null): void {
    if (this.streamId && this.streamId !== streamId && this.retiredStreams.size < MAX_RETIRED_IDENTITIES) {
      this.retiredStreams.add(this.streamId);
    }
    this.streamId = streamId;
    this.runId = null;
    this.retiredRuns.clear();
    this.recoveryRequired = false;
    this.recoveryRequested = false;
  }

  isRetiredStream(value: unknown): boolean {
    const stream = identity(value);
    return !!stream && stream !== this.streamId && this.retiredStreams.has(stream);
  }

  accepts(event: StreamIdentityEvent): boolean {
    const stream = identity(event.streamId);
    const run = identity(event.runId);
    const starts = event.type === 'assistant_run_start';
    const opensStream = starts || (event.type === 'status' && event.isStreaming === true)
      || event.type === 'session_user_message';
    if (stream && stream !== this.streamId && this.retiredStreams.has(stream)) return false;
    if (run && this.retiredRuns.has(run) && !(run === this.runId
      && (event.type === 'status' || event.type === 'token_usage' || event.type === 'context_usage'))) return false;
    // 旧协议仅在尚未观察到明确流身份时兼容；不能给无身份旧消息贴当前标签。
    const sessionOnly = event.type === 'compaction_start' || event.type === 'compaction_end';
    if (!stream && this.streamId && !sessionOnly) return false;
    if (stream && this.streamId && stream !== this.streamId) {
      if (!opensStream) return false;
      if (this.authoritativeSwitchOnly || this.retiredStreams.size >= MAX_RETIRED_IDENTITIES) {
        this.authoritativeSwitchOnly = true; this.recoveryRequired = true; return false;
      }
      this.retiredStreams.add(this.streamId);
    }
    if (run && this.runId && run !== this.runId) {
      if (!starts) return false;
      if (this.retiredRuns.size >= MAX_RETIRED_IDENTITIES) { this.recoveryRequired = true; return false; }
      this.retiredRuns.add(this.runId);
    }
    if (event.type === 'assistant_run_end' && run) {
      if (this.retiredRuns.size >= MAX_RETIRED_IDENTITIES) { this.recoveryRequired = true; return false; }
      this.retiredRuns.add(run);
    }
    if (stream) this.streamId = stream;
    if (run && (starts || !this.runId)) this.runId = run;
    return true;
  }
}
