/** 只执行宿主已持久授权的原生效果，不持有模型、会话或权限决策。 */
import { createHash } from 'node:crypto';
import { SDK_RECENT_DISPATCHES, sdkCommandDigest } from './sdk-dispatch-window.ts';
import type { BridgeEffectCapabilities, BridgeEffectReceipt, BridgeEffectRequest, BridgeStreamCapability } from '../../contracts/generated/ts/lingxi-protocol.ts';
type NativeAdapter = Record<string, any>;
type Scope = 'dm' | 'group';
type Stream = { binding: string; sequence: number; mode: BridgeStreamCapability | null; draftId: number; native: any; lastText: string; preview: boolean; done: boolean; unknown: boolean; typing: boolean; waitingText: boolean };
type Proof = { ids: string[]; ack: boolean; skipped?: string };
class EffectError extends Error {}
function valueText(value: unknown, max = 1024, empty = false): string { if (typeof value !== 'string' || (!empty && !value.trim()) || Buffer.byteLength(value) > max || value.includes('\0')) throw new EffectError('invalid_effect_text'); return value; }
function uint(value: unknown): number { if (!Number.isInteger(value) || Number(value) <= 0 || Number(value) > 0xffff_ffff) throw new EffectError('invalid_effect_sequence'); return Number(value); }
function scopes(value: any): Scope[] { return Array.isArray(value) ? value.filter(item => item === 'dm' || item === 'group') : ['dm']; }
function apiAck(platform: string, value: any): boolean { return value === true || (platform === 'feishu' && (value?.code === 0 || value?.code === '0' || value?.data?.code === 0)) || (platform === 'wechat' && (value?.ret === 0 || value?.errcode === 0)); }
function messageIds(state: any): string[] { return [state?.previousMessageId, state?.messageId].filter(value => typeof value === 'string' && !!value.trim()); }
export function sdkEffectCapabilities(adapter: NativeAdapter): BridgeEffectCapabilities {
  const receipt = adapter?.receiptCapabilities;
  const candidates = [adapter?.streamingCapabilities, adapter?.richStreamingCapabilities].flat().filter(Boolean);
  const streams: BridgeStreamCapability[] = candidates.filter(cap => ['batch', 'draft', 'rich_draft', 'edit_message', 'cardkit_stream'].includes(cap.mode)).map(cap => ({ mode: cap.mode, scopes: scopes(cap.scopes), minIntervalMs: Math.max(500, Math.min(60_000, Number(cap.minIntervalMs) || 500)), maxChars: Math.max(1, Math.min(100_000, Number(cap.maxChars) || 4096)), rich: cap.requiresRichStreaming === true, receiptFolded: cap.receiptMode === 'fold_into_stream' }));
  return { receipt: { mode: receipt?.mode === 'native_typing' ? 'native_typing' : 'text', scopes: receipt ? scopes(receipt.scopes) : ['dm', 'group'], refreshIntervalMs: receipt?.mode === 'native_typing' ? Math.max(0, Math.min(60_000, Number(receipt.refreshIntervalMs) || 0)) : 0, cancellable: receipt?.cancellable === true }, streams };
}
/** 正式调用须先经过 SdkDispatchWindow：全程旧帧由序号水位拒绝，近期结果才留在本类。 */
export class SdkEffects {
  readonly capabilities: BridgeEffectCapabilities;
  private streams = new Map<string, Stream>();
  private retiredStreams = new Map<string, Stream>();
  private receipts = new Map<string, { binding: string; receipt: BridgeEffectReceipt }>();
  private platform: string;
  private adapter: NativeAdapter;
  private idsOf: (platform: any, value: any) => string[] | null;
  private stopped: () => boolean;
  constructor(platform: string, adapter: NativeAdapter, idsOf: (platform: any, value: any) => string[] | null, stopped: () => boolean) {
    this.platform = platform; this.adapter = adapter; this.idsOf = idsOf; this.stopped = stopped;
    this.capabilities = sdkEffectCapabilities(adapter);
  }
  get lifecycleUsage() { return { activeStreams: this.streams.size, retiredStreams: this.retiredStreams.size, effectReceipts: this.receipts.size, nativeHandles: [...this.streams.values(), ...this.retiredStreams.values()].filter(state => state.native || state.preview).length }; }
  private selected(effect: BridgeEffectRequest): BridgeStreamCapability | null { const scope = effect.replyContext.isGroup ? 'group' : 'dm'; const valid = this.capabilities.streams.filter(cap => cap.scopes.includes(scope) && cap.mode !== 'batch'); return (effect.rich ? valid.find(cap => cap.rich) : undefined) || valid.find(cap => !cap.rich) || null; }
  private validate(input: unknown): BridgeEffectRequest {
    if (!input || typeof input !== 'object' || Array.isArray(input)) throw new EffectError('invalid_effect');
    const effect = input as BridgeEffectRequest;
    const allowed = ['effectId', 'streamId', 'inboxId', 'runId', 'sequence', 'operation', 'deliveryId', 'conversationId', 'replyContext', 'text', 'rich'];
    if (Object.keys(effect).some(key => !allowed.includes(key))) throw new EffectError('unexpected_effect_field');
    for (const name of ['effectId', 'streamId', 'inboxId', 'runId', 'conversationId'] as const) valueText(effect[name]);
    uint(effect.sequence); valueText(effect.text, 100_000, true);
    if (!['receipt_start', 'receipt_stop', 'stream_update', 'stream_finish', 'stream_cancel'].includes(effect.operation) || typeof effect.rich !== 'boolean' || !effect.replyContext || typeof effect.replyContext.isGroup !== 'boolean') throw new EffectError('invalid_effect_scope');
    if (effect.deliveryId != null) valueText(effect.deliveryId);
    if ((effect.operation === 'stream_finish') !== (effect.deliveryId != null)) throw new EffectError('final_delivery_identity_required');
    if (effect.operation === 'stream_finish' && (!effect.deliveryId || !effect.text.trim())) throw new EffectError('final_delivery_identity_required');
    return effect;
  }
  async execute(input: unknown): Promise<BridgeEffectReceipt> {
    const effect = this.validate(input);
    const digest = sdkCommandDigest(effect);
    const previous = this.receipts.get(effect.effectId);
    if (previous) { if (previous.binding !== digest) throw new EffectError('effect_identity_conflict'); return previous.receipt; }
    const binding = sdkCommandDigest([effect.inboxId, effect.runId, effect.conversationId, effect.replyContext, effect.rich]);
    let state = this.streams.get(effect.streamId) || this.retiredStreams.get(effect.streamId);
    if (!state) {
      if (effect.sequence === 1 && this.streams.size >= 128) throw new EffectError('stream_capacity_exhausted');
      state = { binding, sequence: 0, mode: this.selected(effect), draftId: (createHash('sha256').update(effect.streamId).digest().readUInt32BE(0) & 0x7fff_ffff) || 1, native: null, lastText: '', preview: false, done: false, unknown: false, typing: false, waitingText: false };
      // 新外围进程不能凭旧序号重建曾发送的卡片/草稿；宿主同样必须把旧stream标未知。
      if (effect.sequence !== 1) { state.unknown = true; state.done = true; }
      else this.streams.set(effect.streamId, state);
    }
    if (state.binding !== binding || effect.sequence <= state.sequence) throw new EffectError('stream_binding_or_sequence_mismatch');
    state.sequence = effect.sequence;
    let invoked = false;
    const invoke = async (fn: () => Promise<any>) => { if (this.stopped()) throw new EffectError('worker_unavailable_before_effect'); invoked = true; return await fn(); };
    let status: BridgeEffectReceipt['status'] = 'confirmed'; let ids: string[] = []; let ack = false; let error: string | null = null;
    try {
      if (state.unknown && !['receipt_stop', 'stream_cancel'].includes(effect.operation)) { status = 'unknown'; error = 'stream_outcome_unconfirmed'; }
      else {
        if (state.done && !['receipt_stop', 'stream_cancel'].includes(effect.operation)) throw new EffectError('stream_already_finished');
        const proof = await this.perform(effect, state, invoke); ids = proof.ids; ack = proof.ack;
        if (proof.skipped) { status = 'skipped'; error = proof.skipped; }
        else if (!ack) { status = 'unknown'; error = 'native_effect_ack_unconfirmed'; }
      }
    } catch (cause) { status = invoked ? 'unknown' : 'failed'; ids = messageIds(state.native); error = cause instanceof EffectError ? cause.message : 'native_effect_failed_or_unconfirmed'; }
    const independentReceipt = effect.operation === 'receipt_stop' || (effect.operation === 'receipt_start' && !state.mode?.receiptFolded);
    if (status === 'unknown' && !independentReceipt) state.unknown = true;
    if (['stream_finish', 'stream_cancel'].includes(effect.operation) || (effect.operation === 'receipt_stop' && !state.mode)) state.done = true;
    const receipt: BridgeEffectReceipt = { effectId: effect.effectId, streamId: effect.streamId, sequence: effect.sequence, operation: effect.operation, deliveryId: effect.deliveryId ?? null, status, platformAck: ack && status === 'confirmed', platformMessageIds: ids, error };
    this.receipts.set(effect.effectId, { binding: digest, receipt });
    while (this.receipts.size > SDK_RECENT_DISPATCHES) this.receipts.delete(this.receipts.keys().next().value!);
    if (state.done) {
      this.streams.delete(effect.streamId);
      const resolvedContent = ['stream_finish', 'stream_cancel'].includes(effect.operation) && status !== 'unknown';
      if (resolvedContent || !state.unknown) { state.native = null; state.lastText = ''; state.preview = false; }
      this.retiredStreams.delete(effect.streamId); this.retiredStreams.set(effect.streamId, state);
      while (this.retiredStreams.size > SDK_RECENT_DISPATCHES) this.retiredStreams.delete(this.retiredStreams.keys().next().value!);
    }
    return receipt;
  }
  private async perform(effect: BridgeEffectRequest, state: Stream, invoke: (fn: () => Promise<any>) => Promise<any>): Promise<Proof> {
    const context = effect.replyContext;
    const target = context.targetChatId || effect.conversationId;
    const options = { ...context, replyContext: context, draftId: state.draftId, targetType: context.targetType || (context.isGroup ? 'group' : 'user') };
    const scope = context.isGroup ? 'group' : 'dm';
    if (effect.operation === 'receipt_start') {
      if (!this.capabilities.receipt.scopes.includes(scope)) return { ids: [], ack: false, skipped: 'receipt_scope_unsupported' };
      if (state.mode?.receiptFolded && ['edit_message', 'cardkit_stream'].includes(state.mode.mode)) return this.updateStream(effect.text || '正在准备回复…', state, target, options, invoke);
      if (this.capabilities.receipt.mode === 'native_typing') {
        if (typeof this.adapter.sendTypingIndicator !== 'function') return { ids: [], ack: false, skipped: 'native_receipt_unsupported' };
        state.typing = true;
        const response = await invoke(() => this.adapter.sendTypingIndicator(target, options)); return { ids: [], ack: apiAck(this.platform, response) };
      }
      if (state.waitingText) return { ids: [], ack: false, skipped: 'waiting_text_already_sent' };
      if (!effect.text.trim()) throw new EffectError('waiting_receipt_text_required');
      state.waitingText = true;
      const response = await invoke(() => this.adapter.sendReply(target, effect.text, options)); const ids = this.idsOf(this.platform, response); return { ids: ids || [], ack: !!ids?.length };
    }
    if (effect.operation === 'receipt_stop') {
      if (!state.typing || !this.capabilities.receipt.cancellable || typeof this.adapter.cancelTypingIndicator !== 'function') return { ids: [], ack: false, skipped: 'native_receipt_not_cancellable' };
      const response = await invoke(() => this.adapter.cancelTypingIndicator(target, options)); state.typing = false; return { ids: [], ack: apiAck(this.platform, response) };
    }
    if (effect.operation === 'stream_cancel') {
      if (!state.preview) { state.done = true; return { ids: [], ack: false, skipped: 'no_native_preview_to_cancel' }; }
      let response;
      if (state.mode?.mode === 'draft' || state.mode?.mode === 'rich_draft') response = await invoke(() => this.adapter[state.mode!.mode === 'rich_draft' ? 'sendRichDraft' : 'sendDraft'](target, '', options));
      else if (state.mode?.mode === 'cardkit_stream' && state.native) response = await invoke(() => this.adapter.finishRichStreamReply(target, state.native, `${state.lastText}\n\n[回复已停止]`, options));
      else if (state.mode?.mode === 'edit_message' && state.native?.messageId) response = await invoke(() => this.adapter.finishStreamReply(target, state.native, `${state.lastText}\n\n[回复已停止]`, options));
      else return { ids: [], ack: false, skipped: 'native_preview_cleanup_unsupported' };
      state.done = true; return { ids: messageIds(state.native), ack: apiAck(this.platform, response) };
    }
    if (!state.mode) return { ids: [], ack: false, skipped: 'native_streaming_scope_unsupported' };
    if (effect.operation === 'stream_update') return this.updateStream(effect.text, state, target, options, invoke);
    if (effect.operation === 'stream_finish') {
      let response;
      if (state.mode.mode === 'draft' || state.mode.mode === 'rich_draft') {
        // 草稿只是预览，最终仍须真实发送取得平台消息编号；任何失败都不回落重发。
        response = await invoke(() => this.adapter[state.mode!.mode === 'rich_draft' ? 'sendRichReply' : 'sendReply'](target, effect.text, options));
        const ids = this.idsOf(this.platform, response); state.done = true; return { ids: ids || [], ack: !!ids?.length };
      }
      if (!state.native) { const started = await this.updateStream(effect.text, state, target, options, invoke); if (!started.ack) return started; }
      response = await invoke(() => this.adapter[state.mode!.mode === 'cardkit_stream' ? 'finishRichStreamReply' : 'finishStreamReply'](target, state.native, effect.text, options));
      state.lastText = effect.text; state.done = true; const ids = messageIds(state.native); return { ids, ack: !!ids.length && apiAck(this.platform, response) };
    }
    throw new EffectError('unknown_effect_operation');
  }
  private async updateStream(text: string, state: Stream, target: string, options: Record<string, any>, invoke: (fn: () => Promise<any>) => Promise<any>): Promise<Proof> {
    const cap = state.mode; if (!cap) return { ids: [], ack: false, skipped: 'native_streaming_scope_unsupported' };
    if (!text.trim() || [...text].length > cap.maxChars) throw new EffectError('native_stream_text_outside_limit');
    let response;
    if (cap.mode === 'draft' || cap.mode === 'rich_draft') {
      // 请求超时仍可能已经显示；固定 draftId 允许宿主随后明确授权清理该预览。
      state.preview = true; state.lastText = text;
      response = await invoke(() => this.adapter[cap.mode === 'rich_draft' ? 'sendRichDraft' : 'sendDraft'](target, text, options)); return { ids: [], ack: apiAck(this.platform, response) };
    }
    if (!['edit_message', 'cardkit_stream'].includes(cap.mode)) return { ids: [], ack: false, skipped: 'native_stream_mode_unsupported' };
    if (!state.native) {
      state.native = await invoke(() => this.adapter[cap.mode === 'cardkit_stream' ? 'startRichStreamReply' : 'startStreamReply'](target, text, { ...options, captureNativeState: native => { state.native = native; state.preview = true; state.lastText = text; } })); state.preview = true; state.lastText = text;
      const ids = messageIds(state.native); return { ids, ack: !!ids.length && !state.native?.missingMessageId };
    }
    response = await invoke(() => this.adapter[cap.mode === 'cardkit_stream' ? 'updateRichStreamReply' : 'updateStreamReply'](target, state.native, text, options)); state.lastText = text; return { ids: messageIds(state.native), ack: apiAck(this.platform, response) };
  }
  /** 外围进程退出不自行补发或清理网络效果；取消命令必须由宿主持久授权。 */
  clear() { this.streams.clear(); this.retiredStreams.clear(); this.receipts.clear(); }
}
