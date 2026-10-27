// 即梦单操作包只做现役 SDK 的能力解析和参数构造；不启动 CLI、不读文件、不拿凭据。
import { parseDreaminaCapabilityOutputs, parseDreaminaVersion } from '../plugins/jimeng-cli/lib/dreamina-capabilities.ts';
import { buildImageSubmitArgs, buildVideoSubmitArgs, parseDreaminaTaskOutput } from '../plugins/jimeng-cli/adapters/dreamina.ts';

const MAX_REQUEST = 10 * 1024 * 1024;
const MAX_OUTPUT = 1024 * 1024;
const OPS = new Set(['jimeng.capabilities', 'jimeng.submit-plan', 'jimeng.parse-result']);
const chunks: Buffer[] = [];
let received = 0;
let accepted = false;
let finished = false;
function finish(value: unknown): void {
  if (finished) return;
  finished = true;
  process.stdout.write(JSON.stringify(value) + '\n', () => process.exit(0));
}
function object(value: unknown, keys: string[]): asserts value is Record<string, any> {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(key => !keys.includes(key))) throw new Error('media_worker_invalid_arguments');
}
function execute(op: string, args: any): unknown {
  if (op === 'jimeng.capabilities') {
    object(args, ['outputs', 'versionOutput', 'fingerprint']);
    object(args.outputs, ['text2image', 'image2image', 'text2video', 'image2video']);
    for (const mode of ['text2image', 'image2image', 'text2video', 'image2video']) {
      if (typeof args.outputs[mode] !== 'string' || !args.outputs[mode] || Buffer.byteLength(args.outputs[mode]) > 2 * 1024 * 1024) throw new Error('media_worker_invalid_capability_help');
    }
    if (typeof args.versionOutput !== 'string' || Buffer.byteLength(args.versionOutput) > 8192) throw new Error('media_worker_invalid_version');
    object(args.fingerprint, ['path', 'mtimeMs', 'size']);
    if (typeof args.fingerprint.path !== 'string' || args.fingerprint.path.length > 8192 || !Number.isFinite(args.fingerprint.mtimeMs) || !Number.isSafeInteger(args.fingerprint.size) || args.fingerprint.size <= 0) throw new Error('media_worker_invalid_fingerprint');
    const version = parseDreaminaVersion(args.versionOutput);
    return { providerId: 'jimeng-cli', fingerprint: { ...args.fingerprint, version: version.version }, version, media: parseDreaminaCapabilityOutputs(args.outputs) };
  }
  if (op === 'jimeng.submit-plan') {
    object(args, ['kind', 'snapshot', 'params', 'defaults']);
    if (!['image', 'video'].includes(args.kind)) throw new Error('media_worker_invalid_kind');
    object(args.params, ['prompt', 'providerId', 'modelId', 'model', 'image', 'referenceImages', 'images', 'mode', 'ratio', 'resolution', 'duration', 'seconds', 'resolution_type', 'resolutionType', 'aspect_ratio', 'aspectRatio', 'video_resolution', 'videoResolution']);
    if (typeof args.params.prompt !== 'string' || !args.params.prompt.trim() || args.params.prompt.length > 32_000) throw new Error('media_worker_invalid_prompt');
    if (!args.snapshot || args.snapshot.providerId !== 'jimeng-cli' || !args.snapshot.media) throw new Error('media_worker_capability_snapshot_missing');
    const defaults = args.defaults ?? {};
    const context = { config: { get: (key: string) => key === 'providerDefaults' ? { 'jimeng-cli': defaults } : undefined }, videoConfig: { get: (key: string) => key === 'providerDefaults' ? { 'jimeng-cli': defaults } : undefined } };
    const argv = args.kind === 'image' ? buildImageSubmitArgs(args.params, context, args.snapshot) : buildVideoSubmitArgs(args.params, context, args.snapshot);
    if (argv.length > 64 || argv.some((value: unknown) => typeof value !== 'string' || value.includes('\0') || value.length > 128 * 1024)) throw new Error('media_worker_plan_exceeds_limit');
    return { argv };
  }
  object(args, ['output']);
  if (typeof args.output !== 'string' || Buffer.byteLength(args.output) > 2 * 1024 * 1024) throw new Error('media_worker_invalid_result');
  return parseDreaminaTaskOutput(args.output);
}
process.stdin.on('data', (chunk: Buffer) => {
  if (accepted) return;
  received += chunk.length;
  if (received > MAX_REQUEST) process.exit(2);
  chunks.push(chunk);
  if (!chunk.includes(10)) return;
  accepted = true;
  process.stdin.pause();
  const bytes = Buffer.concat(chunks, received);
  chunks.length = 0;
  const newline = bytes.indexOf(10);
  let id = '';
  try {
    if (bytes.subarray(newline + 1).length) throw new Error('media_worker_extra_frame');
    const request = JSON.parse(bytes.subarray(0, newline).toString('utf8'));
    id = typeof request.id === 'string' ? request.id : '';
    if (request.proto !== 'lingxi-worker-rpc/1' || request.kind !== 'request' || !id || id.length > 256 || !OPS.has(request.op) || request.resources?.length !== 0) throw new Error('media_worker_invalid_request');
    if (!Number.isSafeInteger(request.deadline_unix_ms) || request.deadline_unix_ms <= Date.now()) throw new Error('media_worker_deadline_exceeded');
    const structured = execute(request.op, request.args);
    const response = { kind: 'result', id, ok: true, content: [], structured, claimed_files: [] };
    if (!Number.isSafeInteger(request.max_output_bytes) || Buffer.byteLength(JSON.stringify(response)) > Math.min(MAX_OUTPUT, request.max_output_bytes)) throw new Error('media_worker_output_limit');
    finish(response);
  } catch {
    // CLI 原始输出可能夹带登录信息；错误只给稳定代码，不带原文、参数或堆栈。
    finish({ kind: 'result', id, ok: false, error: 'media_worker_parse_failed', content: [], claimed_files: [] });
  }
});
process.stdin.on('end', () => { if (!accepted) process.exit(2); });
