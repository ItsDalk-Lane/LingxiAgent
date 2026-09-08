import { describe, expect, it } from 'vitest';
import { createSystemSpeechUtf8Decoder } from '../core/speech-recognition/system-speech-adapter.ts';

function decode(chunks: Buffer[]): string {
  const decoder = createSystemSpeechUtf8Decoder();
  return chunks.map(chunk => decoder.write(chunk)).join('') + decoder.end();
}

describe('R07 system speech UTF-8 chunk decoder', () => {
  it('R07-01：你好在每个 UTF-8 字节边界二分都与整块解码相同', () => {
    const bytes = Buffer.from(JSON.stringify({ ok: true, protocol: 2, text: '你好' }) + '\n', 'utf8');
    const expected = decode([bytes]);
    for (let split = 1; split < bytes.length; split += 1) {
      expect(decode([bytes.subarray(0, split), bytes.subarray(split)])).toBe(expected);
    }
  });

  it('R07-02/06：emoji、组合字符、中英混排与合法 U+FFFD 无损还原', () => {
    const text = 'Lingxi 灵犀 🙂 é � end';
    const bytes = Buffer.from(text, 'utf8');
    expect(decode(Array.from(bytes, (_, index) => bytes.subarray(index, index + 1)))).toBe(text);
  });

  it('R07-03：逐字节与固定种子多段输入收敛到同一字符串', () => {
    const bytes = Buffer.from(JSON.stringify({ ok: true, protocol: 2, text: '固定种子🙂中文' }), 'utf8');
    const bytewise = Array.from(bytes, (_, index) => bytes.subarray(index, index + 1));
    let seed = 20260908;
    const chunks: Buffer[] = [];
    for (let offset = 0; offset < bytes.length;) {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      const size = 1 + (seed % 7);
      chunks.push(bytes.subarray(offset, offset + size));
      offset += size;
    }
    expect(decode(chunks)).toBe(decode(bytewise));
    expect(decode(chunks)).toBe(bytes.toString('utf8'));
  });

  it('R07-07：两个并发输出流的解码状态互不污染', () => {
    const leftBytes = Buffer.from('左🙂通道', 'utf8');
    const rightBytes = Buffer.from('右🌸通道', 'utf8');
    const left = createSystemSpeechUtf8Decoder();
    const right = createSystemSpeechUtf8Decoder();
    let leftText = '';
    let rightText = '';
    const rounds = Math.max(leftBytes.length, rightBytes.length);
    for (let index = 0; index < rounds; index += 1) {
      if (index < leftBytes.length) leftText += left.write(leftBytes.subarray(index, index + 1));
      if (index < rightBytes.length) rightText += right.write(rightBytes.subarray(index, index + 1));
    }
    leftText += left.end();
    rightText += right.end();
    expect(leftText).toBe('左🙂通道');
    expect(rightText).toBe('右🌸通道');
  });
});
