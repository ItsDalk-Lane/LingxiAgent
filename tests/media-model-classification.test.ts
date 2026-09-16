import { describe, expect, it } from 'vitest';
import { mediaModelCapabilities, isMediaOnlyModel } from '../shared/media-model-classification.ts';

describe('媒体模型共用分类', () => {
  it.each([
    [{ id: 'new-2027', inputs: ['text'], outputs: ['image'] }, ['imageGeneration']],
    [{ id: 'new-2027', outputs: ['video'] }, ['videoGeneration']],
    [{ id: 'new-2027', inputs: ['text'], outputs: ['audio'] }, ['speechGeneration']],
    [{ id: 'new-2027', inputs: ['audio'], outputs: ['text'] }, ['speechRecognition']],
    [{ id: 'whisper-next', inputs: ['text', 'audio'], outputs: ['text'] }, ['speechRecognition']],
    [{ id: 'speech-next' }, ['speechGeneration']],
    [{ id: 'qwen3-audio', inputs: ['text', 'audio'], outputs: ['text'] }, []],
    [{ id: 'vision-chat', inputs: ['text', 'image', 'video'], outputs: ['text'] }, []],
    [{ id: 'seedream-next', outputs: ['text'] }, []],
  ])('按实际声明区分用途：%j', (model, expected) => {
    expect(mediaModelCapabilities(model)).toEqual(expected);
  });

  it('混合文本输出同时保留聊天和媒体用途，用户选择覆盖旧目录', () => {
    const model = { id: 'multi-next', outputs: ['text', 'image', 'audio'] };
    expect(mediaModelCapabilities(model)).toEqual(['imageGeneration', 'speechGeneration']);
    expect(isMediaOnlyModel(model)).toBe(false);
    expect(mediaModelCapabilities({ id: 'old', outputs: ['video'] }, { type: 'image', outputs: ['image'] }))
      .toEqual(['videoGeneration']);
    expect(isMediaOnlyModel({ id: 'old', outputs: ['text'] }, { type: 'image' })).toBe(false);
  });
});
