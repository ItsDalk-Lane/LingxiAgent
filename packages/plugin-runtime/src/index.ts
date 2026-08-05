import type {
  PluginResourceDescriptor,
  PluginResourceEdit,
  PluginResourceListItem,
  PluginResourceListResult,
  PluginResourceMaterializeResult,
  PluginResourceMoveResult,
  PluginResourceReadResult,
  PluginResourceRef,
  PluginResourceSearchMatch,
  PluginResourceSearchOptions,
  PluginResourceSearchResult,
  PluginResourceStat,
  PluginResourceTrashOptions,
  PluginResourceTrashResult,
  PluginResourceVersion,
  PluginResourceWatchTarget,
  PluginResourceWriteConflictResult,
  PluginResourceWriteExpectedVersionResult,
  PluginResourceMutationResult,
} from '@lingxi/plugin-protocol';

export type MaybePromise<T> = T | Promise<T>;

export type JsonSchema = Record<string, unknown>;

export const HANA_BUS_SKIP = Symbol.for('lingxi.event-bus.skip');

export interface LingxiToolResult {
  content?: Array<Record<string, unknown>>;
  details?: Record<string, unknown>;
}

export interface LingxiSessionRef {
  sessionId: string;
  sessionPath?: string | null;
  legacySessionPath?: string | null;
}

export type LingxiSessionTarget = string | LingxiSessionRef | {
  sessionId?: string | null;
  sessionPath?: string | null;
  path?: string | null;
  legacySessionPath?: string | null;
};

export interface LingxiSessionFile {
  id?: string | null;
  fileId?: string | null;
  sessionId?: string | null;
  sessionPath?: string | null;
  filePath?: string;
  realPath?: string;
  displayName?: string;
  filename?: string;
  label?: string;
  ext?: string | null;
  mime?: string;
  size?: number;
  kind?: string;
  isDirectory?: boolean;
  origin?: string;
  operations?: unknown[];
  createdAt?: number | string;
  storageKind?: string;
  status?: string;
  missingAt?: number | string | null;
  resource?: LingxiResourceEnvelope;
  [key: string]: unknown;
}

export interface LingxiResourceEnvelope {
  schemaVersion: 1;
  resourceId: string;
  name: string;
  studioId: string;
  type: 'file' | string;
  source: 'session_file' | string;
  sourceId?: string;
  fileId?: string;
  displayName?: string;
  filename?: string;
  ext?: string | null;
  mime?: string;
  size?: number | null;
  kind?: string;
  isDirectory?: boolean;
  origin?: string;
  operations?: string[];
  createdAt?: number | string;
  mtimeMs?: number;
  lifecycle: {
    status: string;
    missingAt: number | string | null;
  };
  storage: {
    provider: string;
    storageKind?: string;
    localOnly?: boolean;
  };
  links: {
    self: string;
    content?: string;
  };
  [key: string]: unknown;
}

export type LingxiResourceRef = PluginResourceRef;
export type LingxiResourceVersion = PluginResourceVersion;
export type LingxiResourceDescriptor = PluginResourceDescriptor;
export type LingxiResourceStat = PluginResourceStat;
export type LingxiResourceReadResult = PluginResourceReadResult;
export type LingxiResourceMutationResult = PluginResourceMutationResult;
export type LingxiResourceWriteConflictResult = PluginResourceWriteConflictResult;
export type LingxiResourceWriteExpectedVersionResult = PluginResourceWriteExpectedVersionResult;
export type LingxiResourceMoveResult = PluginResourceMoveResult;
export type LingxiResourceTrashOptions = PluginResourceTrashOptions;
export type LingxiResourceTrashResult = PluginResourceTrashResult;
export type LingxiResourceEdit = PluginResourceEdit;
export type LingxiResourceListItem = PluginResourceListItem;
export type LingxiResourceListResult = PluginResourceListResult;
export type LingxiResourceSearchOptions = PluginResourceSearchOptions;
export type LingxiResourceSearchMatch = PluginResourceSearchMatch;
export type LingxiResourceSearchResult = PluginResourceSearchResult;
export type LingxiResourceMaterializeResult = PluginResourceMaterializeResult;
export type LingxiResourceWatchTarget = PluginResourceWatchTarget;

export interface LingxiPluginResourceMutationOptions {
  emit?: boolean;
}

export interface LingxiPluginResourceWatchOptions {
  purpose?: string | null;
  sessionRef?: LingxiSessionRef | { sessionPath?: string | null; path?: string | null } | null;
  /** @deprecated Prefer sessionId/sessionRef on the invocation context. */
  sessionPath?: string | null;
}

export interface LingxiResourceWatchSubscription {
  subscriptionId: string;
  resourceKeys: string[];
  unsubscribe(): boolean;
  close(): boolean;
}

export interface LingxiPluginResources {
  stat(ref: LingxiResourceRef | Record<string, unknown>): Promise<LingxiResourceStat>;
  read(ref: LingxiResourceRef | Record<string, unknown>): Promise<LingxiResourceReadResult>;
  list(ref: LingxiResourceRef | Record<string, unknown>): Promise<LingxiResourceListResult>;
  search(ref: LingxiResourceRef | Record<string, unknown>, options?: LingxiResourceSearchOptions): Promise<LingxiResourceSearchResult>;
  materialize(ref: LingxiResourceRef | Record<string, unknown>): Promise<LingxiResourceMaterializeResult>;
  write(ref: LingxiResourceRef | Record<string, unknown>, content: string | Uint8Array | ArrayBuffer, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceMutationResult>;
  writeExpectedVersion(ref: LingxiResourceRef | Record<string, unknown>, content: string | Uint8Array | ArrayBuffer, expectedVersion: LingxiResourceVersion, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceWriteExpectedVersionResult>;
  edit(ref: LingxiResourceRef | Record<string, unknown>, edits: LingxiResourceEdit[], options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceMutationResult>;
  mkdir(ref: LingxiResourceRef | Record<string, unknown>, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceMutationResult>;
  delete(ref: LingxiResourceRef | Record<string, unknown>, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceMutationResult>;
  copy(from: LingxiResourceRef | Record<string, unknown>, to: LingxiResourceRef | Record<string, unknown>, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceMutationResult>;
  rename(from: LingxiResourceRef | Record<string, unknown>, to: LingxiResourceRef | Record<string, unknown>, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceMoveResult>;
  move(from: LingxiResourceRef | Record<string, unknown>, to: LingxiResourceRef | Record<string, unknown>, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceMoveResult>;
  trash(ref: LingxiResourceRef | Record<string, unknown>, trashOptions?: LingxiResourceTrashOptions, options?: LingxiPluginResourceMutationOptions): Promise<LingxiResourceTrashResult>;
  watch(ref: LingxiResourceRef | Record<string, unknown>, options?: LingxiPluginResourceWatchOptions): LingxiResourceWatchSubscription;
  subscribe(resources: Array<LingxiResourceRef | Record<string, unknown>>, options?: LingxiPluginResourceWatchOptions): LingxiResourceWatchSubscription;
  resolveWatchTarget?(ref: LingxiResourceRef | Record<string, unknown>, options?: LingxiPluginResourceWatchOptions): LingxiResourceWatchTarget;
}

export interface LingxiExecutionBoundary {
  schemaVersion: 1;
  boundaryId: string;
  kind: 'local_process' | string;
  serverNodeId: string;
  studioId: string;
  workbench?: {
    kind: string;
    root: string | null;
    [key: string]: unknown;
  };
  sandbox?: {
    kind: string;
    enforcedBy?: string;
    [key: string]: unknown;
  };
  filesystem?: {
    policy: string;
    [key: string]: unknown;
  };
  network?: {
    policy: string;
    [key: string]: unknown;
  };
  [key: string]: unknown;
}

export interface LingxiSessionFileMediaItem {
  type: 'session_file';
  fileId: string;
  sessionId?: string | null;
  sessionPath?: string | null;
  filePath?: string;
  label?: string;
  mime?: string;
  size?: number;
  kind?: string;
  [key: string]: unknown;
}

export interface LingxiStagedSessionFile {
  file?: LingxiSessionFile | null;
  sessionFile?: LingxiSessionFile | null;
  mediaItem: LingxiSessionFileMediaItem;
}

export interface LingxiMediaDetails {
  media: {
    items: LingxiSessionFileMediaItem[];
  };
}

export interface LingxiChatSurfaceCardOptions {
  title?: string;
  description?: string;
  mode?: 'transcript' | 'full' | string;
  composer?: boolean;
  aspectRatio?: string;
}

export interface LingxiChatSurfaceCardDetails {
  type: 'chat.surface';
  pluginId: string;
  sessionId: string;
  sessionRef: LingxiSessionRef;
  sessionPath?: string;
  title?: string;
  description: string;
  mode: 'transcript' | 'full' | string;
  composer?: boolean;
  aspectRatio?: string;
}

export interface LingxiPluginNetworkFetchInit extends RequestInit {
  timeoutMs?: number;
  cacheTtlMs?: number;
  maxResponseBytes?: number;
}

export interface LingxiPluginNetwork {
  fetch(input: string | URL | Request, init?: LingxiPluginNetworkFetchInit): Promise<Response>;
}

export interface LingxiToolContext {
  serverId: string;
  serverNodeId?: string;
  userId: string;
  studioId: string;
  connectionKind?: 'local' | 'lan' | 'custom_remote' | 'relay' | 'cloud' | string;
  credentialKind?: 'none' | 'loopback_token' | 'device_credential' | 'user_session' | string;
  platformAccountId?: string | null;
  officialServiceKind?: 'relay' | 'cloud_studio' | 'inference' | 'billing' | string | null;
  executionBoundary?: LingxiExecutionBoundary;
  pluginId: string;
  pluginDir: string;
  dataDir: string;
  capabilities?: string[];
  sensitiveCapabilities?: string[];
  sessionId?: string | null;
  sessionRef?: LingxiSessionRef | null;
  /** @deprecated Use sessionId/sessionRef. Kept for legacy plugins. */
  sessionPath?: string | null;
  bus: LingxiEventBus;
  network: LingxiPluginNetwork;
  resources: LingxiPluginResources;
  config: LingxiPluginConfigStore;
  log: LingxiPluginLogger;
  registerSessionFile?: (input: Record<string, unknown>) => LingxiSessionFile;
  stageFile?: (input: Record<string, unknown>) => LingxiStagedSessionFile;
  [key: string]: unknown;
}

export type LingxiToolSessionPermissionKind =
  | 'read'
  | 'read_only'
  | 'plugin_output'
  | 'session_file_output'
  | 'workspace_write'
  | 'external_side_effect'
  | 'review'
  | string;

export type LingxiToolInvocationKind = 'read' | 'routine' | 'review';

export type LingxiToolInvocationTargetType =
  | 'url'
  | 'browser_tab'
  | 'background_task'
  | 'channel'
  | 'channel_draft'
  | 'agent'
  | 'notification_route'
  | 'setting'
  | 'memory_store'
  | 'pinned_memory_item'
  | 'pinned_memory_query'
  | 'experience_category'
  | 'session_files'
  | 'terminal_process';

export interface LingxiToolInvocationTarget {
  type: LingxiToolInvocationTargetType;
  /** Exact wildcard-free identity, limited by the host to 4096 characters. */
  id: string;
  /** Display-only label for reviewer context. */
  label?: string;
}

export interface LingxiToolInvocationDescriptor {
  action: string;
  kind: LingxiToolInvocationKind;
  /** Stable capability id in the form `<tool-name>.<action>`. */
  capability: string;
  target?: LingxiToolInvocationTarget;
  sideEffect?: Record<string, unknown>;
}

export interface LingxiToolSessionPermission<Input = unknown> {
  /**
   * True means the tool only reads already-authorized data and may run in
   * read-only sessions without reviewer escalation.
   */
  readOnly?: boolean;
  /**
   * Host approval classification hint. Unknown or external side-effect kinds
   * remain reviewer-bound in Auto mode.
   */
  kind?: LingxiToolSessionPermissionKind;
  /**
   * Override Auto-mode handling for a declared non-read tool.
   */
  auto?: 'allow' | 'review';
  description?: string;
  sideEffect?: Record<string, unknown>;
  describeSideEffect?: (input: Input) => Record<string, unknown> | null | undefined;
  /**
   * Synchronously classify one concrete invocation. Return null for an
   * unsupported action or invalid target so the host can fail closed.
   * Promise/thenable results are consumed safely and rejected. The descriptor
   * action is the resolver's stable permission action; the host does not infer
   * it from an optional input.action field or require those strings to match.
   *
   * Actor, server, and session identity are host-owned and must not appear in
   * the returned descriptor or sideEffect metadata.
   */
  resolveInvocation?: (input: Input) => LingxiToolInvocationDescriptor | null;
}

export interface LingxiToolDefinition<Input = unknown, Output = unknown> {
  name: string;
  description: string;
  parameters?: JsonSchema;
  promptSnippet?: string;
  promptGuidelines?: string;
  sessionPermission?: LingxiToolSessionPermission<Input>;
  metadata?: Record<string, unknown>;
  invocationStyle?: 'sdk_tool' | 'pi_tool';
  execute(input: Input, ctx: LingxiToolContext): MaybePromise<Output>;
}

export type LingxiSlashPermission = 'anyone' | 'owner' | 'admin';
export type LingxiSlashScope = 'session' | 'global';

export interface LingxiCommandContext {
  [key: string]: unknown;
}

export interface LingxiCommandResult {
  reply?: string;
  silent?: boolean;
  error?: string;
  [key: string]: unknown;
}

export interface LingxiCommandDefinition<Context = LingxiCommandContext> {
  name: string;
  aliases?: string[];
  description?: string;
  scope?: LingxiSlashScope;
  permission?: LingxiSlashPermission;
  usage?: string;
  handler?: (ctx: Context) => MaybePromise<LingxiCommandResult | void>;
  execute?: (ctx: Context) => MaybePromise<unknown>;
}

export type LingxiProviderRuntimeKind = 'http' | 'oauth-http' | 'local-cli' | 'browser-cli' | 'plugin';
export type LingxiMediaCapabilityName = 'imageGeneration' | 'videoGeneration' | 'speechGeneration' | string;
export type LingxiMediaOutputKind = 'file_glob' | 'json_stdout' | 'url_stdout';
export type LingxiCliBindingSource = 'prompt' | 'modelId' | 'inputFile' | 'outputDir' | 'size' | 'duration';

export type LingxiCliArgBinding =
  | { literal: string }
  | { option: string; from: LingxiCliBindingSource };

export interface LingxiCliOutputContract {
  kind: LingxiMediaOutputKind;
  directory?: LingxiCliBindingSource | string;
  pattern?: string;
  [key: string]: unknown;
}

export interface LingxiCliCommandSpec {
  executable: string;
  args: LingxiCliArgBinding[];
  timeoutMs: number;
  output: LingxiCliOutputContract;
}

export interface LingxiProviderRuntime {
  kind: LingxiProviderRuntimeKind;
  protocolId?: string;
  command?: LingxiCliCommandSpec;
  [key: string]: unknown;
}

export interface LingxiProviderChatCapability {
  projection?: 'models-json' | 'sdk-auth-alias' | 'none' | string;
  credentialSource?: 'provider-catalog' | 'auth-storage' | 'none';
  runtimeProviderId?: string;
  displayProviderId?: string;
  allowListSource?: string;
  [key: string]: unknown;
}

export interface LingxiMediaReferenceImageLimits {
  min?: number;
  max?: number;
  [key: string]: unknown;
}

export interface LingxiMediaInputLimits {
  referenceImages?: LingxiMediaReferenceImageLimits;
  [key: string]: unknown;
}

export interface LingxiProviderMediaMode {
  id: string;
  label?: string;
  parameterSchema?: JsonSchema;
  defaults?: Record<string, unknown>;
  inputLimits?: LingxiMediaInputLimits;
  pricing?: Record<string, unknown>;
  agentHints?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface LingxiProviderMediaModel {
  id: string;
  displayName?: string;
  protocolId: string;
  inputs?: string[];
  outputs?: string[];
  supportsEdit?: boolean;
  aliases?: string[];
  credentialLaneId?: string;
  modes?: LingxiProviderMediaMode[];
  parameterSchema?: JsonSchema;
  defaults?: Record<string, unknown>;
  inputLimits?: LingxiMediaInputLimits;
  [key: string]: unknown;
}

export interface LingxiProviderCredentialLane {
  id: string;
  kind?: string;
  label?: string;
  [key: string]: unknown;
}

export interface LingxiProviderMediaCapability {
  defaultModelId?: string;
  models: LingxiProviderMediaModel[];
  credentialLanes?: LingxiProviderCredentialLane[];
  [key: string]: unknown;
}

export interface LingxiProviderCapabilities {
  chat?: LingxiProviderChatCapability;
  media?: Partial<Record<LingxiMediaCapabilityName, LingxiProviderMediaCapability>>;
  [key: string]: unknown;
}

export interface LingxiProviderSource {
  kind: 'builtin' | 'plugin' | 'user' | string;
  pluginId?: string;
  [key: string]: unknown;
}

export interface LingxiProviderDefinition {
  id: string;
  displayName?: string;
  name?: string;
  authType?: 'api-key' | 'oauth' | 'none' | string;
  authJsonKey?: string;
  defaultBaseUrl?: string;
  defaultApi?: string;
  api?: string;
  models?: unknown[];
  runtime?: LingxiProviderRuntime;
  capabilities?: LingxiProviderCapabilities;
  source?: LingxiProviderSource;
  [key: string]: unknown;
}

export type LingxiExtensionFactory<Pi = unknown> = (pi: Pi) => MaybePromise<void>;

export interface LingxiPluginConfigStore {
  get<T = unknown>(key: string, options?: LingxiPluginConfigScopeOptions): MaybePromise<T | undefined>;
  getAll?(options?: LingxiPluginConfigScopeOptions & { redacted?: boolean }): MaybePromise<Record<string, unknown>>;
  set<T = unknown>(key: string, value: T, options?: LingxiPluginConfigScopeOptions): MaybePromise<void>;
  setMany?(values: Record<string, unknown>, options?: LingxiPluginConfigScopeOptions): MaybePromise<Record<string, unknown>>;
  getSchema?(): JsonSchema;
}

export interface LingxiPluginConfigScopeOptions {
  scope?: 'global' | 'per-agent' | 'per-session';
  agentId?: string;
  sessionId?: string;
  /** @deprecated Use sessionId. Kept for legacy config scopes. */
  sessionPath?: string;
}

export interface LingxiSessionTurnContext {
  system?: string | Array<string | { text: string; label?: string }>;
  beforeUser?: string | Array<string | { text: string; label?: string }>;
  afterUser?: string | Array<string | { text: string; label?: string }>;
  metadata?: Record<string, unknown>;
}

export interface LingxiSessionCreateInput {
  agentId?: string | null;
  cwd?: string | null;
  memoryEnabled?: boolean;
  model?: string | { id?: string; modelId?: string; provider?: string; providerId?: string };
  workspaceFolders?: string[];
  authorizedFolders?: string[];
  thinkingLevel?: string;
  permissionMode?: string;
  ownerPluginId?: string | null;
  kind?: string | null;
  sessionKind?: string | null;
  visibility?: 'public' | 'plugin_private' | 'private' | string;
}

export interface LingxiSessionSendInput {
  text: string;
  context?: LingxiSessionTurnContext | null;
  images?: unknown[];
  videos?: unknown[];
  audios?: unknown[];
  imageAttachmentPaths?: string[];
  videoAttachmentPaths?: string[];
  audioAttachmentPaths?: string[];
  [key: string]: unknown;
}

export interface LingxiSessionListFilter {
  agentId?: string;
  ownerPluginId?: string;
  includePluginPrivate?: boolean;
}

export interface LingxiSessionUpdateInput {
  title?: string;
  pinned?: boolean;
  projectId?: string | null;
  thinkingLevel?: string;
  permissionMode?: string;
  ownerPluginId?: string | null;
  kind?: string | null;
  visibility?: 'public' | 'plugin_private' | 'private' | string;
}

export interface LingxiAgentCreateInput {
  id?: string;
  name: string;
  yuan?: string;
  ownerPluginId?: string | null;
  visibility?: 'public' | 'plugin_private' | 'private' | string;
  kind?: string | null;
  initialFiles?: Record<string, string>;
  initialMemory?: Record<string, unknown>;
  memoryPolicy?: { enabled?: boolean };
}

export interface LingxiAgentUpdateInput {
  name?: string;
  yuan?: string;
  ownerPluginId?: string | null;
  visibility?: 'public' | 'plugin_private' | 'private' | string;
  kind?: string | null;
  memoryPolicy?: { enabled?: boolean };
  toolPolicy?: { disabled?: string[] };
  config?: Record<string, unknown>;
}

export interface LingxiModelSampleInput {
  systemPrompt?: string;
  messages: Array<{ role: string; content: unknown }>;
  sessionId?: string;
  sessionRef?: LingxiSessionRef;
  /** @deprecated Use sessionId/sessionRef. */
  sessionPath?: string;
  agentId?: string;
  temperature?: number;
  maxTokens?: number;
  operation?: string;
}

export interface LingxiMediaProviderFilter {
  capability?: string;
}

export interface LingxiMediaModelRef {
  providerId?: string;
  provider?: string;
  modelId?: string;
  model?: string;
  capability?: string;
  credentialLaneId?: string;
}

export type LingxiSessionFileReference =
  | { kind: 'session_file'; fileId: string }
  | { type: 'session_file'; fileId: string };

export type LingxiGenerateImageReference = LingxiSessionFileReference;

export interface LingxiMediaDelivery {
  mode?: 'session' | 'response' | string;
  ttlMs?: number;
  [key: string]: unknown;
}

export interface LingxiGenerateImageInput {
  sessionId?: string;
  sessionRef?: LingxiSessionRef;
  /** @deprecated Use sessionId/sessionRef. */
  sessionPath?: string;
  prompt: string;
  count?: number;
  image?: LingxiGenerateImageReference | LingxiGenerateImageReference[];
  referenceImages?: LingxiGenerateImageReference[];
  ratio?: string;
  resolution?: string;
  quality?: string;
  mode?: string;
  options?: Record<string, unknown>;
  model?: string;
  provider?: string;
  input?: Record<string, unknown>;
  metadata?: Record<string, unknown>;
  delivery?: LingxiMediaDelivery;
  deliveryMode?: string;
  deliveryTarget?: unknown;
}

export interface LingxiGenerateVideoInput {
  sessionId?: string;
  sessionRef?: LingxiSessionRef;
  /** @deprecated Use sessionId/sessionRef. */
  sessionPath?: string;
  prompt: string;
  image?: LingxiGenerateImageReference | LingxiGenerateImageReference[] | string;
  referenceImages?: LingxiGenerateImageReference[];
  duration?: number;
  ratio?: string;
  resolution?: string;
  mode?: string;
  options?: Record<string, unknown>;
  model?: string;
  provider?: string;
  input?: Record<string, unknown>;
  metadata?: Record<string, unknown>;
  delivery?: LingxiMediaDelivery;
  deliveryMode?: string;
  deliveryTarget?: unknown;
}

export interface LingxiGenerateMediaInput {
  kind?: 'image' | 'video' | 'audio' | 'image_generation' | 'video_generation' | 'speech_recognition' | 'asr' | 'transcription' | string;
  type?: string;
  mediaKind?: string;
  sessionId?: string;
  sessionRef?: LingxiSessionRef;
  /** @deprecated Use sessionId/sessionRef. */
  sessionPath?: string;
  fileId?: string;
  prompt?: string;
  image?: LingxiGenerateImageReference | LingxiGenerateImageReference[] | string;
  referenceImages?: LingxiGenerateImageReference[];
  duration?: number;
  ratio?: string;
  resolution?: string;
  quality?: string;
  mode?: string;
  options?: Record<string, unknown>;
  model?: string;
  provider?: string;
  delivery?: LingxiMediaDelivery;
  deliveryMode?: string;
  input?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface LingxiTranscribeAudioInput {
  sessionId?: string;
  sessionRef?: LingxiSessionRef;
  /** @deprecated Use sessionId/sessionRef. */
  sessionPath?: string;
  fileId: string;
  language?: string;
  providerId?: string;
  provider?: string;
  modelId?: string;
  model?: string;
}

export interface LingxiTranscribeAudioResult {
  ok: true;
  transcription: unknown;
  taskId?: string;
  stream?: unknown;
}

export interface LingxiEventBus {
  emit(event: unknown, sessionPath?: string | null): unknown;
  emit(type: string, payload?: unknown): unknown;
  subscribe(callback: (event: unknown, sessionPath?: string | null) => void, filter?: LingxiBusSubscriptionFilter): () => void;
  subscribe(type: string, handler: (payload: unknown) => void): () => void;
  request<T = unknown>(type: string, payload?: unknown, options?: Record<string, unknown>): Promise<T>;
  hasHandler?(type: string): boolean;
  handle?(type: string, handler: (payload: unknown) => MaybePromise<unknown>): () => void;
  listCapabilities?(): LingxiEventBusCapability[];
  getCapability?(type: string): LingxiEventBusCapability | null;
}

export interface LingxiPluginRouteRequestContext {
  pluginId: string;
  agentId: string | null;
  principal: Record<string, unknown> | null;
  capabilityGrant: {
    accessLevel: string;
    declaredPermissions: readonly string[];
    legacyDeclaration: boolean;
  };
  bus: Pick<LingxiEventBus, 'request' | 'emit' | 'subscribe' | 'hasHandler' | 'getCapability' | 'listCapabilities'>;
}

export interface LingxiPluginHonoLikeContext {
  get?(name: string): unknown;
}

export function getPluginRequestContext(c: LingxiPluginHonoLikeContext): LingxiPluginRouteRequestContext {
  if (!c || typeof c.get !== 'function') {
    throw new Error('getPluginRequestContext requires a Hono context with c.get(name)');
  }
  const requestContext = c.get('pluginRequestContext');
  if (!requestContext || typeof requestContext !== 'object') {
    throw new Error('getPluginRequestContext must be called inside a Hana plugin route handler');
  }
  const bus = (requestContext as Record<string, unknown>).bus;
  const request = bus && typeof bus === 'object'
    ? (bus as { request?: unknown }).request
    : null;
  if (typeof request !== 'function') {
    throw new Error('getPluginRequestContext found an invalid plugin route request context');
  }
  return requestContext as LingxiPluginRouteRequestContext;
}

export interface LingxiBusSubscriptionFilter {
  types?: string[] | Set<string>;
  [key: string]: unknown;
}

export interface LingxiEventBusCapability {
  type: string;
  title: string;
  description: string;
  inputSchema: JsonSchema;
  outputSchema: JsonSchema;
  permission: string;
  errors: string[];
  stability: string;
  owner: string;
  since?: string;
  available?: boolean;
}

export interface LingxiNormalizedUsage {
  input: {
    totalTokens: number | null;
    uncachedTokens: number | null;
  };
  output: {
    totalTokens: number | null;
    reasoningTokens: number | null;
  };
  cache: {
    readTokens: number | null;
    writeTokens: number | null;
    missTokens: number | null;
    hit: boolean | null;
    created: boolean | null;
    hitRatio: number | null;
    support: 'reported' | 'not_reported' | 'not_supported';
  };
  totalTokens: number | null;
  costTotal: number | null;
}

export type LingxiUsageAttribution =
  | { kind: 'session'; agentId: string | null; sessionId?: string | null; sessionPath?: string | null }
  | { kind: 'phone_conversation'; agentId: string; conversationId: string; conversationType: 'channel' | 'dm'; sessionId?: string | null; sessionPath?: string | null }
  | { kind: 'memory'; agentId: string | null }
  | { kind: 'automation'; jobId?: string | null; runId?: string | null; agentId?: string | null }
  | { kind: 'plugin'; pluginId: string; agentId?: string | null; sessionId?: string | null; sessionPath?: string | null }
  | { kind: 'utility'; agentId?: string | null; sessionId?: string | null; sessionPath?: string | null }
  | { kind: 'unknown' };

export interface LingxiUsageSource {
  subsystem: 'session' | 'phone' | 'memory' | 'automation' | 'subagent' | 'compaction' | 'plugin' | 'utility' | 'vision' | 'unknown' | string;
  operation: string;
  surface: 'desktop' | 'mobile' | 'bridge' | 'channel' | 'dm' | 'cron' | 'heartbeat' | 'system' | 'plugin' | 'unknown' | string;
  trigger: 'user' | 'manual' | 'threshold' | 'overflow' | 'daily' | 'scheduled' | 'startup' | 'tool' | 'unknown' | string;
  actor?: {
    kind: 'session' | 'phone_conversation' | 'automation' | 'plugin' | 'subagent' | 'unknown' | string;
    agentId?: string | null;
    sessionId?: string | null;
    sessionPath?: string | null;
    taskId?: string | null;
    [key: string]: unknown;
  };
  parent?: {
    kind: 'session' | 'phone_conversation' | 'automation' | 'plugin' | 'unknown' | string;
    sessionId?: string;
    sessionPath?: string;
    conversationId?: string;
    conversationType?: 'channel' | 'dm';
    taskId?: string;
    pluginId?: string;
    [key: string]: unknown;
  };
}

export interface LingxiUsageLedgerEntry {
  schemaVersion: 1;
  requestId: string;
  startedAt: string;
  endedAt: string | null;
  durationMs: number | null;
  status: 'ok' | 'error' | 'aborted' | 'usage_missing';
  source: LingxiUsageSource;
  attribution: LingxiUsageAttribution;
  model: {
    provider: string | null;
    modelId: string | null;
    api: string | null;
  };
  usage: LingxiNormalizedUsage | null;
  rawUsageShape: string | null;
  error: {
    name: string | null;
    message: string | null;
  } | null;
}

export interface LingxiUsageListFilter {
  since?: string;
  until?: string;
  attributionKind?: string;
  sessionId?: string;
  sessionPath?: string;
  agentId?: string;
  subsystem?: string;
  operation?: string;
  modelId?: string;
  provider?: string;
  status?: 'ok' | 'error' | 'aborted' | 'usage_missing' | string;
  limit?: number;
}

export interface LingxiUsageListResult {
  entries: LingxiUsageLedgerEntry[];
  nextCursor: string | null;
}

export interface LingxiUsageEventMeta {
  sessionId?: string | null;
  sessionPath?: string | null;
  sessionRef?: LingxiSessionRef | null;
}

export interface LingxiPluginLogger {
  debug(...args: unknown[]): void;
  info(...args: unknown[]): void;
  warn(...args: unknown[]): void;
  error(...args: unknown[]): void;
}

export interface LingxiBusHandlerContext {
  serverId: string;
  serverNodeId?: string;
  userId: string;
  studioId: string;
  connectionKind?: 'local' | 'lan' | 'custom_remote' | 'relay' | 'cloud' | string;
  credentialKind?: 'none' | 'loopback_token' | 'device_credential' | 'user_session' | string;
  platformAccountId?: string | null;
  officialServiceKind?: 'relay' | 'cloud_studio' | 'inference' | 'billing' | string | null;
  executionBoundary?: LingxiExecutionBoundary;
  pluginId: string;
  bus: LingxiEventBus;
  network?: LingxiPluginNetwork;
  resources?: LingxiPluginResources;
  config?: LingxiPluginConfigStore;
  log?: LingxiPluginLogger;
  [key: string]: unknown;
}

export interface LingxiBusHandlerDefinition<
  Payload = unknown,
  Result = unknown,
  Context extends LingxiBusHandlerContext = LingxiBusHandlerContext,
> {
  type: string;
  handle(payload: Payload, ctx: Context): MaybePromise<Result>;
}

export interface LingxiPluginContext {
  serverId: string;
  serverNodeId?: string;
  userId: string;
  studioId: string;
  connectionKind?: 'local' | 'lan' | 'custom_remote' | 'relay' | 'cloud' | string;
  credentialKind?: 'none' | 'loopback_token' | 'device_credential' | 'user_session' | string;
  platformAccountId?: string | null;
  officialServiceKind?: 'relay' | 'cloud_studio' | 'inference' | 'billing' | string | null;
  executionBoundary?: LingxiExecutionBoundary;
  pluginId: string;
  pluginDir: string;
  dataDir: string;
  capabilities?: string[];
  sensitiveCapabilities?: string[];
  sessionId?: string | null;
  sessionRef?: LingxiSessionRef | null;
  /** @deprecated Use sessionId/sessionRef. Kept for legacy plugins. */
  sessionPath?: string | null;
  bus: LingxiEventBus;
  network: LingxiPluginNetwork;
  resources: LingxiPluginResources;
  config: LingxiPluginConfigStore;
  log: LingxiPluginLogger;
  registerTool?: (tool: LingxiToolDefinition) => () => void;
  registerSessionFile?: (input: Record<string, unknown>) => LingxiSessionFile;
  stageFile?: (input: Record<string, unknown>) => LingxiStagedSessionFile;
  [key: string]: unknown;
}

export type LingxiPluginDisposable = () => void;

export interface LingxiPluginLifecycleHelpers {
  register(disposable: LingxiPluginDisposable): void;
}

export interface LingxiPluginLifecycle {
  onload?(ctx: LingxiPluginContext, helpers: LingxiPluginLifecycleHelpers): MaybePromise<void>;
  onunload?(ctx: LingxiPluginContext): MaybePromise<void>;
}

export interface LingxiPluginInstance {
  ctx: LingxiPluginContext;
  register: (disposable: LingxiPluginDisposable) => void;
  onload?(): MaybePromise<void>;
  onunload?(): MaybePromise<void>;
}

export type LingxiTaskStatus =
  | 'pending'
  | 'running'
  | 'paused'
  | 'blocked'
  | 'recovering'
  | 'completed'
  | 'failed'
  | 'canceled'
  | 'aborted';

export interface LingxiTaskProgress {
  current?: number;
  total?: number;
  percent?: number;
  message?: string;
}

export interface LingxiTaskRecord {
  taskId: string;
  type: string;
  parentSessionPath?: string | null;
  pluginId?: string | null;
  agentId?: string | null;
  meta?: Record<string, unknown>;
  progress?: LingxiTaskProgress | null;
  status: LingxiTaskStatus;
  aborted?: boolean;
  createdAt?: number;
  updatedAt?: number;
  completedAt?: number;
  result?: unknown;
  error?: string;
}

export interface LingxiTaskSchedule {
  scheduleId: string;
  type: string;
  pluginId?: string | null;
  agentId?: string | null;
  parentSessionPath?: string | null;
  payload?: unknown;
  meta?: Record<string, unknown>;
  intervalMs?: number | null;
  runAt?: number | string | null;
  enabled?: boolean;
  nextRunAt?: number | null;
  lastRunAt?: number | null;
  lastResult?: unknown;
  lastError?: string | null;
  runCount?: number;
}

export interface LingxiTaskRegisterInput {
  taskId: string;
  type: string;
  parentSessionPath?: string | null;
  pluginId?: string | null;
  agentId?: string | null;
  meta?: Record<string, unknown>;
  persist?: boolean;
}

export interface LingxiTaskUpdateInput {
  taskId: string;
  status?: LingxiTaskStatus;
  progress?: LingxiTaskProgress | null;
  meta?: Record<string, unknown>;
  result?: unknown;
  error?: unknown;
  parentSessionPath?: string | null;
  pluginId?: string | null;
  agentId?: string | null;
}

export interface LingxiTaskScheduleInput {
  scheduleId: string;
  type: string;
  pluginId?: string | null;
  agentId?: string | null;
  parentSessionPath?: string | null;
  payload?: unknown;
  meta?: Record<string, unknown>;
  intervalMs?: number;
  runAt?: number | string | Date;
  enabled?: boolean;
}

const EMPTY_PARAMETERS: JsonSchema = { type: 'object', properties: {} };

export function defineTool<Input = unknown, Output = unknown>(
  definition: LingxiToolDefinition<Input, Output>,
): LingxiToolDefinition<Input, Output> & { parameters: JsonSchema } {
  return {
    ...definition,
    parameters: definition.parameters ?? EMPTY_PARAMETERS,
  };
}

export function defineCommand<Context = LingxiCommandContext>(
  definition: LingxiCommandDefinition<Context>,
): LingxiCommandDefinition<Context> {
  return { ...definition };
}

export function defineProvider<T extends LingxiProviderDefinition>(definition: T): T {
  return definition;
}

export function defineBusHandler<
  Payload = unknown,
  Result = unknown,
  Context extends LingxiBusHandlerContext = LingxiBusHandlerContext,
>(
  definition: LingxiBusHandlerDefinition<Payload, Result, Context>,
): LingxiBusHandlerDefinition<Payload, Result, Context> {
  return { ...definition };
}

export function requestBus<Result = unknown, Payload = unknown>(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  type: string,
  payload?: Payload,
  options?: Record<string, unknown>,
): Promise<Result> {
  if (!ctx.bus || typeof ctx.bus.request !== 'function') {
    throw new Error('plugin bus request unavailable');
  }
  return ctx.bus.request<Result>(type, payload, options);
}

function pluginIdFromContext(ctx: { pluginId?: string | null }): string | null {
  return typeof ctx.pluginId === 'string' && ctx.pluginId.length > 0 ? ctx.pluginId : null;
}

function withOwnerPlugin<T extends Record<string, unknown>>(
  ctx: { pluginId?: string | null },
  input: T,
): T {
  const pluginId = pluginIdFromContext(ctx);
  if (!pluginId || input.ownerPluginId) return input;
  return { ...input, ownerPluginId: pluginId };
}

function withContextMetadata(
  ctx: { pluginId?: string | null },
  context: LingxiSessionTurnContext | null | undefined,
): LingxiSessionTurnContext | null | undefined {
  const pluginId = pluginIdFromContext(ctx);
  if (!pluginId) return context;
  if (!context) {
    return { metadata: { pluginId } };
  }
  return {
    ...context,
    metadata: {
      pluginId,
      ...(context.metadata || {}),
    },
  };
}

function textOrNull(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function normalizeSessionTarget(target: LingxiSessionTarget): Record<string, unknown> {
  if (typeof target === 'string') return { sessionPath: target };
  if (!target || typeof target !== 'object') return { sessionPath: target as unknown };

  const sessionId = textOrNull((target as any).sessionId);
  const sessionPath = textOrNull((target as any).sessionPath) || textOrNull((target as any).path);
  const legacySessionPath = textOrNull((target as any).legacySessionPath);
  if (!sessionId) {
    return sessionPath ? { sessionPath } : {};
  }

  const sessionRef: LingxiSessionRef = {
    sessionId,
    ...(sessionPath ? { sessionPath } : {}),
    ...(legacySessionPath ? { legacySessionPath } : {}),
  };
  return {
    sessionId,
    ...(sessionPath ? { sessionPath } : {}),
    ...(legacySessionPath ? { legacySessionPath } : {}),
    sessionRef,
  };
}

function sessionRefFromTarget(target: LingxiSessionTarget): LingxiSessionRef | null {
  const payload = normalizeSessionTarget(target);
  return (payload.sessionRef as LingxiSessionRef | undefined) || null;
}

export function createChatSurfaceCard(
  ctx: { pluginId?: string | null },
  target: LingxiSessionTarget,
  options: LingxiChatSurfaceCardOptions = {},
): LingxiChatSurfaceCardDetails {
  const pluginId = pluginIdFromContext(ctx);
  if (!pluginId) {
    throw new Error('createChatSurfaceCard requires ctx.pluginId');
  }
  const payload = normalizeSessionTarget(target);
  const sessionId = textOrNull(payload.sessionId);
  const sessionPath = textOrNull(payload.sessionPath);
  if (!sessionId) {
    throw new Error('createChatSurfaceCard requires sessionId or sessionRef; sessionPath alone is legacy locator metadata');
  }
  const sessionRef: LingxiSessionRef = {
    sessionId,
    ...(sessionPath ? { sessionPath } : {}),
  };
  return {
    type: 'chat.surface',
    pluginId,
    sessionId,
    sessionRef,
    ...(sessionPath ? { sessionPath } : {}),
    ...(options.title ? { title: options.title } : {}),
    description: options.description || 'Plugin private chat session.',
    mode: options.mode || 'transcript',
    ...(options.composer !== undefined ? { composer: options.composer } : {}),
    ...(options.aspectRatio ? { aspectRatio: options.aspectRatio } : {}),
  };
}

export function createSession(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiSessionCreateInput = {},
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'session:create', withOwnerPlugin(ctx, { ...input }), options);
}

export function getSession(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  target: LingxiSessionTarget,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'session:get', normalizeSessionTarget(target), options);
}

export function listSessions(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  filter: LingxiSessionListFilter = {},
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'session:list', filter, options);
}

export function updateSession(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  target: LingxiSessionTarget,
  patch: LingxiSessionUpdateInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'session:update', {
    ...normalizeSessionTarget(target),
    ...withOwnerPlugin(ctx, { ...patch }),
  }, options);
}

export function sendSessionMessage(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  target: LingxiSessionTarget,
  input: LingxiSessionSendInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'session:send', {
    ...normalizeSessionTarget(target),
    ...input,
    context: withContextMetadata(ctx, input.context),
  }, options);
}

export function subscribeSessionEvents(
  ctx: { bus?: Pick<LingxiEventBus, 'subscribe'> | null },
  target: LingxiSessionTarget,
  handler: (event: unknown, meta: { sessionId: string | null; sessionPath: string | null; sessionRef: LingxiSessionRef | null }) => void,
): () => void {
  if (!ctx.bus || typeof ctx.bus.subscribe !== 'function') {
    throw new Error('plugin bus subscribe unavailable');
  }
  const filter = normalizeSessionTarget(target);
  const targetRef = sessionRefFromTarget(target);
  return ctx.bus.subscribe((event, scopedSessionPath) => {
    const eventSessionId = event && typeof event === 'object' ? textOrNull((event as any).sessionId) : null;
    const sessionId = eventSessionId || targetRef?.sessionId || null;
    const sessionPath = scopedSessionPath || targetRef?.sessionPath || null;
    const sessionRef = sessionId ? {
      sessionId,
      ...(sessionPath ? { sessionPath } : {}),
      ...(targetRef?.legacySessionPath ? { legacySessionPath: targetRef.legacySessionPath } : {}),
    } : null;
    handler(event, { sessionId, sessionPath, sessionRef });
  }, filter);
}

export function listAgents(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  filter: { ownerPluginId?: string; includePluginPrivate?: boolean } = {},
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'agent:list', filter, options);
}

export function getAgentProfile(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  agentId: string,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'agent:profile', { agentId }, options);
}

export function createAgent(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiAgentCreateInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'agent:create', withOwnerPlugin(ctx, { ...input }), options);
}

export function updateAgent(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  agentId: string,
  patch: LingxiAgentUpdateInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'agent:update', { agentId, ...withOwnerPlugin(ctx, { ...patch }) }, options);
}

export function sampleText(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiModelSampleInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'model:sample-text', {
    ...input,
    ...(pluginIdFromContext(ctx) ? { pluginId: pluginIdFromContext(ctx) } : {}),
  }, options);
}

export function listMediaProviders(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  filter: LingxiMediaProviderFilter = {},
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'provider:media-providers', filter, options);
}

export function resolveMediaModel(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  ref: LingxiMediaModelRef,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'provider:resolve-media-model', ref, options);
}

export function generateImage(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiGenerateImageInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'media:generate-image', {
    ...input,
    ...(pluginIdFromContext(ctx) ? { pluginId: pluginIdFromContext(ctx) } : {}),
  }, options);
}

export function generateVideo(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiGenerateVideoInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'media:generate-video', {
    ...input,
    ...(pluginIdFromContext(ctx) ? { pluginId: pluginIdFromContext(ctx) } : {}),
  }, options);
}

export function generateMedia(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiGenerateMediaInput,
  options?: Record<string, unknown>,
): Promise<unknown> {
  return requestBus(ctx, 'media:generate', {
    ...input,
    ...(pluginIdFromContext(ctx) ? { pluginId: pluginIdFromContext(ctx) } : {}),
  }, options);
}

export function transcribeAudio(
  ctx: { pluginId?: string | null; bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiTranscribeAudioInput,
  options?: Record<string, unknown>,
): Promise<LingxiTranscribeAudioResult> {
  return requestBus(ctx, 'media:transcribe-audio', {
    ...input,
    ...(pluginIdFromContext(ctx) ? { pluginId: pluginIdFromContext(ctx) } : {}),
  }, options).then(normalizeTranscribeAudioResult);
}

function normalizeTranscribeAudioResult(result: unknown): LingxiTranscribeAudioResult {
  if (result && typeof result === 'object' && (result as any).ok === true
    && Object.prototype.hasOwnProperty.call(result, 'transcription')) {
    return result as LingxiTranscribeAudioResult;
  }
  return { ok: true, transcription: result };
}

export function listUsageEntries(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  filter: LingxiUsageListFilter = {},
  options?: Record<string, unknown>,
): Promise<LingxiUsageListResult> {
  return requestBus<LingxiUsageListResult, LingxiUsageListFilter>(ctx, 'usage:list', filter, options);
}

export function subscribeUsageEvents(
  ctx: { bus?: Pick<LingxiEventBus, 'subscribe'> | null },
  handler: (entry: LingxiUsageLedgerEntry, meta: LingxiUsageEventMeta) => void,
): () => void {
  if (!ctx.bus || typeof ctx.bus.subscribe !== 'function') {
    throw new Error('plugin bus subscribe unavailable');
  }
  return ctx.bus.subscribe((event, sessionPath) => {
    if (!event || typeof event !== 'object') return;
    const typed = event as { type?: unknown; entry?: unknown };
    if (typed.type !== 'llm_usage') return;
    const entry = typed.entry as LingxiUsageLedgerEntry;
    const entrySessionId =
      textOrNull((entry as any)?.attribution?.sessionId)
      || textOrNull((entry as any)?.source?.actor?.sessionId)
      || textOrNull((entry as any)?.source?.parent?.sessionId);
    const entrySessionPath =
      textOrNull((entry as any)?.attribution?.sessionPath)
      || textOrNull((entry as any)?.source?.actor?.sessionPath)
      || textOrNull((entry as any)?.source?.parent?.sessionPath)
      || textOrNull(sessionPath);
    handler(entry, {
      ...(entrySessionId ? { sessionId: entrySessionId } : {}),
      sessionPath: entrySessionPath,
      ...(entrySessionId ? {
        sessionRef: {
          sessionId: entrySessionId,
          ...(entrySessionPath ? { sessionPath: entrySessionPath } : {}),
        },
      } : {}),
    });
  }, { types: ['llm_usage'] });
}

export function registerTask(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiTaskRegisterInput,
): Promise<{ ok: true }> {
  return requestBus(ctx, 'task:register', input);
}

export function updateTask(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiTaskUpdateInput,
): Promise<{ ok: true; task: LingxiTaskRecord }> {
  return requestBus(ctx, 'task:update', input);
}

export function completeTask(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  taskId: string,
  result?: unknown,
): Promise<{ ok: true; task: LingxiTaskRecord }> {
  return requestBus(ctx, 'task:complete', { taskId, result });
}

export function failTask(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  taskId: string,
  error: unknown,
): Promise<{ ok: true; task: LingxiTaskRecord }> {
  return requestBus(ctx, 'task:fail', { taskId, error });
}

export function cancelTask(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  taskId: string,
  reason?: string,
): Promise<{ result: string; canceled: boolean }> {
  return requestBus(ctx, 'task:cancel', { taskId, reason });
}

export function scheduleTask(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  input: LingxiTaskScheduleInput,
): Promise<{ ok: true; schedule: LingxiTaskSchedule }> {
  return requestBus(ctx, 'task:schedule', input);
}

export function unscheduleTask(
  ctx: { bus?: Pick<LingxiEventBus, 'request'> | null },
  scheduleId: string,
): Promise<{ ok: true; removed: boolean }> {
  return requestBus(ctx, 'task:unschedule', { scheduleId });
}

export function sessionFileToMediaItem(file: LingxiSessionFile): LingxiSessionFileMediaItem {
  const fileId = firstText(file.fileId, file.id);
  if (!fileId) {
    throw new Error('SessionFile media item requires id or fileId');
  }

  const item: LingxiSessionFileMediaItem = {
    type: 'session_file',
    fileId,
  };
  assignDefined(item, 'sessionId', file.sessionId);
  assignDefined(item, 'sessionPath', file.sessionPath);
  assignDefined(item, 'filePath', file.filePath);
  assignDefined(item, 'label', firstText(file.label, file.displayName, file.filename));
  assignDefined(item, 'mime', file.mime);
  assignDefined(item, 'size', file.size);
  assignDefined(item, 'kind', file.kind);
  return item;
}

type LingxiMediaInput = LingxiSessionFile | LingxiSessionFileMediaItem | LingxiStagedSessionFile;

export function createMediaDetails(items: LingxiMediaInput[]): LingxiMediaDetails {
  return {
    media: {
      items: items.map(normalizeMediaItem),
    },
  };
}

export function defineExtension<Pi = unknown>(factory: LingxiExtensionFactory<Pi>): LingxiExtensionFactory<Pi> {
  return factory;
}

export function definePlugin(lifecycle: LingxiPluginLifecycle): new () => LingxiPluginInstance {
  return class DefinedLingxiPlugin implements LingxiPluginInstance {
    ctx!: LingxiPluginContext;
    register!: (disposable: LingxiPluginDisposable) => void;

    async onload(): Promise<void> {
      await lifecycle.onload?.(this.ctx, { register: this.register });
    }

    async onunload(): Promise<void> {
      await lifecycle.onunload?.(this.ctx);
    }
  };
}

function normalizeMediaItem(input: LingxiMediaInput): LingxiSessionFileMediaItem {
  if (isRecord(input) && isRecord(input.mediaItem)) {
    return normalizeSessionFileMediaItem(input.mediaItem);
  }
  if (isRecord(input) && input.type === 'session_file') {
    return normalizeSessionFileMediaItem(input);
  }
  if (isRecord(input)) {
    return sessionFileToMediaItem(input);
  }
  throw new Error('media details item must be a SessionFile, staged file, or session_file media item');
}

function normalizeSessionFileMediaItem(input: Record<string, unknown>): LingxiSessionFileMediaItem {
  if (input.type !== 'session_file') {
    throw new Error('media details item must be a session_file media item');
  }
  const fileId = firstText(input.fileId);
  if (!fileId) {
    throw new Error('SessionFile media item requires fileId');
  }
  return {
    ...input,
    type: 'session_file',
    fileId,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function firstText(...values: unknown[]): string | undefined {
  for (const value of values) {
    if (typeof value === 'string' && value.length > 0) {
      return value;
    }
  }
  return undefined;
}

function assignDefined(target: Record<string, unknown>, key: string, value: unknown): void {
  if (value !== undefined && value !== null) {
    target[key] = value;
  }
}
