export type LocalModelCategory = "embedding" | "ocr" | "stt" | "tts";
export type LocalModelTier = "small" | "large";
export type LocalModelBackend = "cpu" | "coreml" | "metal" | "cuda" | "vulkan" | "directml";
export type LocalModelSource = "remote" | "manual";
export type LocalModelIntegrity = "verified" | "unknown";
export type LocalModelPriority = "interactive" | "normal" | "batch";

export interface LocalModelRef {
  id: string;
  quant: string;
  manifestVersion: string;
}

export interface LocalModelDescriptor extends LocalModelRef {
  category: LocalModelCategory;
  tier: LocalModelTier;
  runtimeId: string;
  runtimeVersion: string;
  estimatedPeakRssMb: number;
}

export interface LocalModelDiagnostics {
  protocolId?: string;
  runtimeId?: string;
  runtimeVersion?: string;
  pid?: number;
  peakRssMb?: number;
  [key: string]: unknown;
}

export interface LocalModelResult<T> {
  modelId: string;
  variant: string;
  backend: LocalModelBackend;
  durationMs: number;
  inputBytes: number;
  output: T;
  diagnostics?: LocalModelDiagnostics;
}

export interface LocalEmbeddingOutput {
  vectors: number[][];
  dimensions: number;
  modelKey: string;
}

export interface LocalOcrOutput {
  markdown: string;
  text: string;
  format: "ocr";
  warnings: string[];
}

export interface LocalTranscriptionOutput {
  text: string;
  language?: string;
  durationMs?: number;
}

export interface LocalSynthesisOutput {
  sampleRate: 16000 | 24000;
  format: "wav" | "pcm_s16le";
  audio: Uint8Array;
}

export interface LocalModelCallBase {
  model: LocalModelRef;
  signal: AbortSignal;
  priority?: LocalModelPriority;
}

export interface LocalEmbedRequest extends LocalModelCallBase {
  texts: string[];
  inputType?: "query" | "document";
}

export interface LocalOcrRequest extends LocalModelCallBase {
  image: Uint8Array;
  mime: string;
  language?: string;
}

export interface LocalTranscriptionRequest extends LocalModelCallBase {
  filePath: string;
  mime: string;
  language?: string;
}

export interface LocalSynthesisRequest extends LocalModelCallBase {
  text: string;
  voice?: string;
  sampleRate?: 16000 | 24000;
  onChunk?: (chunk: Uint8Array) => void | Promise<void>;
}

export interface LocalModelRuntime {
  embed(request: LocalEmbedRequest): Promise<LocalModelResult<LocalEmbeddingOutput>>;
  ocr(request: LocalOcrRequest): Promise<LocalModelResult<LocalOcrOutput>>;
  transcribe(request: LocalTranscriptionRequest): Promise<LocalModelResult<LocalTranscriptionOutput>>;
  synthesize(request: LocalSynthesisRequest): Promise<LocalModelResult<LocalSynthesisOutput>>;
}

export interface InstalledLocalModel {
  id: string;
  category: LocalModelCategory;
  quant: string;
  version: string;
  source: LocalModelSource;
  installedAt: string;
  bytes: number;
  sha256Manifest: string;
  integrity: LocalModelIntegrity;
}

export function localModelVariantKey(ref: Pick<LocalModelRef, "id" | "quant">): string {
  return `${ref.id}@${ref.quant}`;
}

export function localModelKey(ref: LocalModelRef): string {
  return `local:${ref.id}@${ref.quant}@${ref.manifestVersion}`;
}

/** 解析持久化在知识/记忆/媒体配置中的稳定本地模型身份。 */
export function parseLocalModelKey(value: unknown): LocalModelRef | null {
  if (typeof value !== "string" || !value.startsWith("local:")) return null;
  const parts = value.slice("local:".length).split("@");
  if (parts.length !== 3 || parts.some((part) => !/^[A-Za-z0-9._-]+$/.test(part))) return null;
  const [id, quant, manifestVersion] = parts;
  return { id, quant, manifestVersion };
}
