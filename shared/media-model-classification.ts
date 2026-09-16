import { readModalityListLoose } from './modality.ts';

export type MediaModelCapability = 'imageGeneration' | 'videoGeneration' | 'speechGeneration' | 'speechRecognition';

type ModelMetadata = { id?: unknown; type?: unknown; inputs?: unknown; outputs?: unknown };

// 名称只补充没有明确能力声明的目录，不覆盖用户选择，也不把视觉输入当生成能力。
const FAMILIES: Array<[RegExp, MediaModelCapability]> = [
  [/gpt-image|dall-e|cogview|seedream|imagen|imagegen|stable-diffusion|ideogram|recraft|qwen-image|wan[\d.]*-image|z-image|(?:^|[-_.])t2i(?:$|[-_.])|(?:^|[-_.])flux(?:$|[-_.])/i, 'imageGeneration'],
  [/(?:^|[-_.])sora(?:$|[-_.])|seedance|(?:^|[-_.])kling|hailuo|(?:^|[-_.])vidu(?:$|[-_.])|pixverse|runway|luma[-_]?(?:dream|ray)|qwen-video|wan[\d.]*-video|videox|(?:^|[-_.])t2v(?:$|[-_.])|(?:^|[-_.])i2v(?:$|[-_.])/i, 'videoGeneration'],
  [/(?:^|[-_./])tts(?:$|[-_.])|(?:^|[-_./])speech(?:$|[-_.])/i, 'speechGeneration'],
  [/whisper|paraformer|sensevoice|fun-asr|qwen-audio-transcribe|livetranslate|(?:^|[-_./])(?:asr|bigasr|transcribe|transcription)(?:$|[-_.])/i, 'speechRecognition'],
];

function declaredKind(type: unknown): MediaModelCapability | null {
  if (type === 'image') return 'imageGeneration';
  if (type === 'video') return 'videoGeneration';
  if (type === 'tts' || type === 'speech_generation') return 'speechGeneration';
  if (type === 'asr' || type === 'transcription' || type === 'speech_recognition') return 'speechRecognition';
  return null;
}

/** 前后端共用媒体分类；返回能力而不是可调用保证，调用方式仍由供应商决定。 */
export function mediaModelCapabilities(model: ModelMetadata, known?: ModelMetadata | null): MediaModelCapability[] {
  const outputs = readModalityListLoose(model.outputs) ?? readModalityListLoose(known?.outputs);
  const inputs = readModalityListLoose(model.inputs) ?? readModalityListLoose(known?.inputs);
  const type = model.type ?? known?.type;
  const explicitKind = declaredKind(type);
  const id = typeof model.id === 'string' ? model.id : '';
  const family = FAMILIES.find(([pattern]) => pattern.test(id))?.[1];
  if (outputs) {
    const capabilities: MediaModelCapability[] = [];
    if (outputs.includes('image')) capabilities.push('imageGeneration');
    if (outputs.includes('video')) capabilities.push('videoGeneration');
    if (outputs.includes('audio')) capabilities.push('speechGeneration');
    const recognition = explicitKind === 'speechRecognition' || family === 'speechRecognition'
      || (inputs?.length === 1 && inputs[0] === 'audio');
    if (outputs.includes('text') && recognition && (!inputs || inputs.includes('audio'))) {
      capabilities.push('speechRecognition');
    }
    return capabilities;
  }
  if (explicitKind) return [explicitKind];
  // 老目录的 audio/speech 类型未区分识别与合成，优先用输入与名称消歧。
  if (type === 'audio' || type === 'speech') {
    return [family === 'speechGeneration' ? family : 'speechRecognition'];
  }
  if (type === 'chat') return [];
  return family ? [family] : [];
}

/** 混合文本输出保留聊天入口；专门的识音模型不会作为普通聊天模型使用。 */
export function isMediaOnlyModel(model: ModelMetadata, known?: ModelMetadata | null): boolean {
  const capabilities = mediaModelCapabilities(model, known);
  if (!capabilities.length) return false;
  const outputs = readModalityListLoose(model.outputs) ?? readModalityListLoose(known?.outputs);
  return !outputs?.includes('text') || capabilities.includes('speechRecognition');
}
