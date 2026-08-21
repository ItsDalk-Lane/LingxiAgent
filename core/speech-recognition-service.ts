import { MediaAdapterRegistry } from "./media-adapter-registry.ts";
import { builtinSpeechRecognitionAdapters } from "./speech-recognition/adapters.ts";
import { createModuleLogger } from "../lib/debug-log.ts";
import fs from "node:fs";
import {
  buildMediaModelEditPatch,
  MediaModelEditValidationError,
  requireExistingMediaModel,
} from "../shared/media-model-edit.ts";
import { withModelRequestAccounting } from "../lib/llm/model-request-accounting.ts";
import {
  beginObservedModelCall,
  failObservedModelCall,
  observedModelCallLedgerMetadata,
} from "../lib/llm/model-call-integration.ts";
import {
  createSemanticInputProvenance,
  provenanceSection,
} from "../lib/llm/semantic-input-provenance.ts";
import { runWithNewModelTrace } from "../lib/llm/model-trace-scope.ts";

const CAPABILITY = "speech_recognition";

/**
 * 输入体积桶（§四十）：只读已有 length/size——一次 stat 元数据读取，
 * 不复制 Buffer、不 hash 音频。
 */
function audioInputSizeBucket(file: any): string | null {
  const filePath = file?.realPath || file?.filePath;
  if (typeof filePath !== "string" || !filePath.trim()) {
    return typeof file?.size === "number" && Number.isFinite(file.size) ? bucketForSize(file.size) : null;
  }
  try {
    return bucketForSize(fs.statSync(filePath).size);
  } catch {
    return null;
  }
}

function bucketForSize(bytes: number): string | null {
  if (!Number.isFinite(bytes) || bytes < 0) return null;
  if (bytes <= 256 * 1024) return "<=256KB";
  if (bytes <= 1024 * 1024) return "<=1MB";
  if (bytes <= 10 * 1024 * 1024) return "<=10MB";
  return ">10MB";
}

function textOrNull(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function normalizeSessionRef(input: any = {}) {
  const rawRef = input.sessionRef && typeof input.sessionRef === "object" ? input.sessionRef : null;
  const sessionId = textOrNull(input.sessionId) || textOrNull(rawRef?.sessionId);
  const sessionPath =
    textOrNull(input.sessionPath)
    || textOrNull(rawRef?.sessionPath)
    || textOrNull(rawRef?.path);
  const legacySessionPath =
    textOrNull(input.legacySessionPath)
    || textOrNull(rawRef?.legacySessionPath)
    || (sessionId && sessionPath ? sessionPath : null);
  const sessionRef = sessionId
    ? {
      sessionId,
      ...(sessionPath ? { sessionPath } : {}),
      ...(legacySessionPath ? { legacySessionPath } : {}),
    }
    : null;
  return { sessionId, sessionPath, sessionRef };
}
const log = createModuleLogger("speech-recognition");

export /** Phase 5：speech 语义输入 provenance 组装（input.audio + parameters.language）。 */
function buildSpeechProvenanceSections(language) {
  const sections = [
    provenanceSection(
      { root: "input", path: ["audio"] },
      "audio_input",
      { role: "input", source: { type: "runtime", id: "speech.audio" } },
    ),
  ];
  if (typeof language === "string" && language.trim()) {
    sections.push(provenanceSection(
      { root: "parameters", path: ["language"] },
      "language_hint",
      { role: "parameter", source: { type: "runtime", id: "speech.language" } },
    ));
  }
  return sections;
}

/** Phase 5 测试入口。 */
export function buildSpeechProvenanceForTest({ language }) {
  return createSemanticInputProvenance("speech_transcribe", buildSpeechProvenanceSections(language));
}

export class SpeechRecognitionService {
  declare _emitEvent: any;
  declare _fetch: any;
  declare _logger: any;
  declare _getUsageLedger: any;
  declare _prefs: any;
  declare _providers: any;
  declare _resolveProviderCredentialsFresh: any;
  declare _registry: any;
  declare _sessionFiles: any;
  constructor({
    providerRegistry,
    resolveProviderCredentialsFresh,
    preferences,
    sessionFiles,
    emitEvent,
    fetch,
    logger = log,
    adapters = builtinSpeechRecognitionAdapters,
    usageLedger = null,
    getUsageLedger = null,
  }: any = {}) {
    if (!providerRegistry) throw new Error("SpeechRecognitionService requires providerRegistry");
    if (typeof resolveProviderCredentialsFresh !== "function") {
      throw new Error("SpeechRecognitionService requires resolveProviderCredentialsFresh");
    }
    if (!preferences) throw new Error("SpeechRecognitionService requires preferences");
    if (!sessionFiles) throw new Error("SpeechRecognitionService requires sessionFiles");
    this._providers = providerRegistry;
    this._resolveProviderCredentialsFresh = resolveProviderCredentialsFresh;
    this._prefs = preferences;
    this._sessionFiles = sessionFiles;
    this._emitEvent = typeof emitEvent === "function" ? emitEvent : () => {};
    this._fetch = fetch;
    this._logger = logger;
    this._getUsageLedger = typeof getUsageLedger === "function" ? getUsageLedger : () => usageLedger;
    this._registry = new MediaAdapterRegistry();
    for (const adapter of adapters || []) this.registerAdapter(adapter);
  }

  registerAdapter(adapter) {
    this._registry.register(adapter);
  }

  unregisterAdapter(adapterId) {
    this._registry.unregister(adapterId);
  }

  hasAdapterForModel(providerId, model) {
    if (!model?.protocolId) return false;
    return Boolean(this._registry.getProtocol(model.protocolId) || this._registry.get(providerId));
  }

  listProviders() {
    const next: any = {};
    for (const provider of this._providers.getMediaProviders(CAPABILITY) || []) {
      const providerId = provider.providerId;
      const models = (provider.models || [])
        .map((model) => ({
          ...model,
          adapterAvailable: this.hasAdapterForModel(providerId, model),
        }))
        .filter((model) => model.adapterAvailable);
      // 候选目录：内置声明模型（未被用户添加），仅用于「添加模型」下拉；
      // availableModels 仍然只含已添加且可运行的模型（默认模型选择的合法集合）。
      const catalogModels = (provider.availableModels || [])
        .filter((model) => this.hasAdapterForModel(providerId, model))
        .map((model) => ({
          id: model.id,
          name: model.displayName || model.name || model.id,
        }));
      if (!models.length && !catalogModels.length) continue;
      const credentialStatus = this._providers.getMediaProviderCredentialStatus?.(providerId, CAPABILITY) || {};
      next[providerId] = {
        ...provider,
        ...credentialStatus,
        models,
        availableModels: models.map((model) => ({
          id: model.id,
          name: model.displayName || model.name || model.id,
        })),
        catalogModels,
      };
    }
    return {
      providers: next,
      config: this.getConfig(),
    };
  }

  setProviderModel(providerId, model) {
    this._providers.addMediaModel(providerId, CAPABILITY, model);
    return { ok: true };
  }

  /**
   * 编辑已添加的语音识别模型（PUT 语义：只改 displayName / inputs / outputs）。
   * 模型必须已存在；runtime-discovered 目录不可人工变更。
   */
  updateProviderModel(providerId, modelId, patch) {
    if (this._providers.getRuntimeMediaCapabilitySourceOwner?.(providerId)) {
      throw new MediaModelEditValidationError(
        `Runtime-discovered provider "${providerId}" does not allow manual model changes`,
      );
    }
    const existing = requireExistingMediaModel({
      models: this._providers.getMediaModels(providerId, CAPABILITY),
      providerId,
      modelId,
      capability: CAPABILITY,
    });
    const safePatch = buildMediaModelEditPatch({ capability: CAPABILITY, body: patch, existingModel: existing });
    this._providers.updateMediaModelEntry(providerId, CAPABILITY, modelId, safePatch);
    return { ok: true };
  }

  removeProviderModel(providerId, modelId) {
    this._providers.removeMediaModel(providerId, CAPABILITY, modelId);
    return { ok: true };
  }

  getConfig() {
    return this._prefs.getSpeechRecognitionConfig?.() || { enabled: false };
  }

  setConfig(patch) {
    const next = normalizeSpeechRecognitionConfigPatch(patch, this.getConfig());
    if (next.defaultModel) {
      const listed = this.listProviders().providers;
      const provider = listed[next.defaultModel.provider];
      if (!provider?.models?.some((model) => model.id === next.defaultModel.id)) {
        throw new Error("speech recognition default model is unavailable");
      }
    }
    return this._prefs.setSpeechRecognitionConfig?.(next) || next;
  }

  async queueVoiceTranscription({ sessionId, sessionPath, sessionRef, fileId, language }: any = {}) {
    Promise.resolve()
      .then(() => this.transcribeVoiceAttachment({ sessionId, sessionPath, sessionRef, fileId, language }))
      .catch((err) => {
        this._logger?.warn?.(`voice transcription queue failed for ${fileId || "(missing fileId)"}: ${err?.message || err}`);
      });
  }

  async transcribeAudio(payload: any = {}) {
    // 独立语音转写 = 新 Trace（§三十八/§二十五）：REST/后台触发的 ASR 是独立
    // 任务；转写结果后续触发的新消息属于新的用户 turn，有自己的 trace。
    return runWithNewModelTrace(
      { origin: "speech", refs: { ...(payload?.fileId ? { fileId: String(payload.fileId) } : {}) } },
      () => this._transcribeAudioWithinTrace(payload),
    );
  }

  async _transcribeAudioWithinTrace(payload: any = {}) {
    const {
      fileId,
      language,
      providerId,
      provider,
      modelId,
      model,
    } = payload;
    const sessionTarget = normalizeSessionRef(payload);
    const { sessionId, sessionPath, sessionRef } = sessionTarget;
    if ((!sessionId && !sessionPath) || !fileId) {
      throw new Error("sessionId or sessionPath and fileId are required for audio transcription");
    }

    const file = this._sessionFiles.get(fileId, { sessionId, sessionPath });
    if (!file) throw new Error(`session file not found: ${fileId}`);

    const providerRef = providerId || provider || null;
    const modelRef = modelId || model || null;
    const config = this.getConfig();
    const defaultModel = config.defaultModel || null;
    const targetProvider = providerRef || defaultModel?.provider || null;
    const targetModel = modelRef || defaultModel?.id || null;
    if (!targetProvider || !targetModel) {
      throw new Error("speech recognition model is not configured");
    }

    const target = this._providers.resolveMediaModel({
      providerId: targetProvider,
      modelId: targetModel,
      capability: CAPABILITY,
    });
    const adapter = this._registry.getProtocol(target.model.protocolId) || this._registry.get(target.providerId);
    if (!adapter?.transcribe) throw new Error(`No speech recognition adapter registered for protocol "${target.model.protocolId}"`);

    const pending = this._updateTranscription({ sessionId, sessionPath }, fileId, {
      status: "pending",
      providerId: target.providerId,
      modelId: target.model.id,
      protocolId: target.model.protocolId,
      ...(language ? { language } : {}),
    });
    this._emitTranscriptionUpdate({ sessionId, sessionPath, sessionRef }, fileId, pending.transcription);

    try {
      const credentials = await this._resolveCredentialsFresh(target);
      const result = await this._transcribeWithAccounting({
        adapter,
        file,
        target,
        credentials,
        language,
        sessionId,
        sessionPath,
        fileId,
      });
      const ready = this._updateTranscription({ sessionId, sessionPath }, fileId, {
        status: "ready",
        text: result.text || "",
        providerId: target.providerId,
        modelId: target.model.id,
        protocolId: target.model.protocolId,
        ...(result.language ? { language: result.language } : language ? { language } : {}),
        ...(result.durationMs !== undefined ? { durationMs: result.durationMs } : {}),
      });
      this._emitTranscriptionUpdate({ sessionId, sessionPath, sessionRef }, fileId, ready.transcription);
      return ready.transcription;
    } catch (err) {
      const failed = this._updateTranscription({ sessionId, sessionPath }, fileId, {
        status: "failed",
        providerId: target.providerId,
        modelId: target.model.id,
        protocolId: target.model.protocolId,
        ...(language ? { language } : {}),
        error: err?.message || String(err),
      });
      this._emitTranscriptionUpdate({ sessionId, sessionPath, sessionRef }, fileId, failed.transcription);
      return failed.transcription;
    }
  }

  async transcribeVoiceAttachment(payload: any = {}) {
    // 同 transcribeAudio：voice 附件转写是独立后台派生任务（fire-and-forget），
    // force-new 防止沿队列异步链继承任何外层 scope（§五十）。
    return runWithNewModelTrace(
      { origin: "speech", refs: { ...(payload?.fileId ? { fileId: String(payload.fileId) } : {}) } },
      () => this._transcribeVoiceAttachmentWithinTrace(payload),
    );
  }

  async _transcribeVoiceAttachmentWithinTrace(payload: any = {}) {
    const { fileId, language } = payload;
    const sessionTarget = normalizeSessionRef(payload);
    const { sessionId, sessionPath, sessionRef } = sessionTarget;
    const config = this.getConfig();
    if (!config.enabled || !config.defaultModel) return { status: "skipped", reason: "disabled" };
    if ((!sessionId && !sessionPath) || !fileId) {
      throw new Error("sessionId or sessionPath and fileId are required for voice transcription");
    }

    const file = this._sessionFiles.get(fileId, { sessionId, sessionPath });
    if (!file) throw new Error(`session file not found: ${fileId}`);
    if (file.presentation !== "voice-input") return { status: "skipped", reason: "not_voice_input" };

    const target = this._providers.resolveMediaModel({
      providerId: config.defaultModel.provider,
      modelId: config.defaultModel.id,
      capability: CAPABILITY,
    });
    const adapter = this._registry.getProtocol(target.model.protocolId) || this._registry.get(target.providerId);
    if (!adapter?.transcribe) throw new Error(`No speech recognition adapter registered for protocol "${target.model.protocolId}"`);

    const pending = this._updateTranscription({ sessionId, sessionPath }, fileId, {
      status: "pending",
      providerId: target.providerId,
      modelId: target.model.id,
      protocolId: target.model.protocolId,
      ...(language ? { language } : {}),
    });
    this._emitTranscriptionUpdate({ sessionId, sessionPath, sessionRef }, fileId, pending.transcription);

    try {
      const credentials = await this._resolveCredentialsFresh(target);
      const result = await this._transcribeWithAccounting({
        adapter,
        file,
        target,
        credentials,
        language,
        sessionId,
        sessionPath,
        fileId,
      });
      const ready = this._updateTranscription({ sessionId, sessionPath }, fileId, {
        status: "ready",
        text: result.text || "",
        providerId: target.providerId,
        modelId: target.model.id,
        protocolId: target.model.protocolId,
        ...(result.language ? { language: result.language } : language ? { language } : {}),
        ...(result.durationMs !== undefined ? { durationMs: result.durationMs } : {}),
      });
      this._emitTranscriptionUpdate({ sessionId, sessionPath, sessionRef }, fileId, ready.transcription);
      return ready.transcription;
    } catch (err) {
      const failed = this._updateTranscription({ sessionId, sessionPath }, fileId, {
        status: "failed",
        providerId: target.providerId,
        modelId: target.model.id,
        protocolId: target.model.protocolId,
        ...(language ? { language } : {}),
        error: err?.message || String(err),
      });
      this._emitTranscriptionUpdate({ sessionId, sessionPath, sessionRef }, fileId, failed.transcription);
      return failed.transcription;
    }
  }

  _updateTranscription(sessionRef, fileId, transcription) {
    return this._sessionFiles.updateTranscription(fileId, transcription, sessionRef);
  }

  async _resolveCredentialsFresh(target) {
    const credentialProviderId = target.credentialLane?.providerId || target.providerId;
    return this._resolveProviderCredentialsFresh(credentialProviderId);
  }

  async _transcribeWithAccounting({
    adapter,
    file,
    target,
    credentials,
    language,
    sessionId,
    sessionPath,
    fileId = null,
  }) {
    // MC-09（§三十七/§三十八）：在 file/provider/model/protocol/language/session
    // 都确定之后、真正 Adapter HTTP 请求之前铸 callId。fileId 是业务引用
    // （不是音频内容），进入安全 attribution。
    // Phase 5（§七十四）：语义输入 = audio + language hint；locator 只指
    // input.audio / parameters.language，不携带字节/base64/路径/转写文本。
    const speechProvenanceSections = buildSpeechProvenanceSections(language);
    const recorder = beginObservedModelCall({
      model: {
        provider: target.providerId,
        modelId: target.model.id,
        api: target.model.protocolId,
      },
      source: {
        subsystem: "speech-recognition",
        operation: "transcribe",
        surface: "media",
        trigger: "user",
      },
      attribution: {
        kind: "session",
        ...(sessionId ? { sessionId } : {}),
        ...(sessionPath ? { sessionPath } : {}),
        ...(fileId ? { fileId } : {}),
      },
      details: {
        path: "speech_transcribe",
        mediaType: "audio",
        protocol: target.model.protocolId,
        languageSpecified: Boolean(language),
        inputSizeBucket: audioInputSizeBucket(file),
      },
      semanticInputProvenance: createSemanticInputProvenance("speech_transcribe", speechProvenanceSections),
    });
    try {
      const result = await withModelRequestAccounting({
        usageLedger: this._getUsageLedger(),
        model: {
          provider: target.providerId,
          modelId: target.model.id,
          api: target.model.protocolId,
        },
        usageContext: {
          source: {
            subsystem: "speech-recognition",
            operation: "transcribe",
            surface: "media",
            trigger: "user",
          },
          attribution: {
            kind: "session",
            ...(sessionId ? { sessionId } : {}),
            ...(sessionPath ? { sessionPath } : {}),
          },
        },
        metadata: { capability: CAPABILITY, ...observedModelCallLedgerMetadata(recorder) },
      }, () => adapter.transcribe({
        file,
        provider: target.provider,
        model: target.model,
        credentials,
        language,
        fetch: this._fetch,
        modelCall: recorder,
      }));
      // 语义响应（§四十二）：只记录结构事实；绝不记录 transcription text。
      recorder.semanticResponseCompleted({
        details: {
          hasText: Boolean(result?.text && String(result.text).trim()),
          languagePresent: Boolean(result?.language),
          durationPresent: result?.durationMs !== undefined,
        },
      });
      recorder.endLogicalCall("ok");
      return result;
    } catch (err) {
      failObservedModelCall(recorder, err, { errorKind: "adapter_error" });
      throw err;
    }
  }

  _emitTranscriptionUpdate(sessionRef, fileId, transcription) {
    const { sessionId, sessionPath } = normalizeSessionRef(sessionRef);
    this._emitEvent({
      type: "voice_transcription_update",
      ...(sessionId ? { sessionId } : {}),
      sessionPath,
      fileId,
      transcription,
    }, sessionPath);
  }
}

function normalizeSpeechRecognitionConfigPatch(patch, current: any = {}) {
  const body = patch && typeof patch === "object" && !Array.isArray(patch) ? patch : {};
  const next = { ...current };
  if (Object.prototype.hasOwnProperty.call(body, "enabled")) {
    next.enabled = body.enabled === true;
  }
  if (Object.prototype.hasOwnProperty.call(body, "defaultModel")) {
    const value = body.defaultModel;
    if (value === null || value === undefined) {
      delete next.defaultModel;
    } else if (value && typeof value === "object" && !Array.isArray(value)) {
      const provider = typeof value.provider === "string" ? value.provider.trim() : "";
      const id = typeof value.id === "string" ? value.id.trim() : "";
      if (!provider || !id) throw new Error("speechRecognition.defaultModel requires provider and id");
      next.defaultModel = { provider, id };
    } else {
      throw new Error("speechRecognition.defaultModel must be an object");
    }
  }
  if (!next.enabled) {
    return {
      enabled: false,
      ...(next.defaultModel ? { defaultModel: next.defaultModel } : {}),
    };
  }
  return {
    enabled: true,
    ...(next.defaultModel ? { defaultModel: next.defaultModel } : {}),
  };
}
