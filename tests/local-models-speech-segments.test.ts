import { describe, expect, it } from "vitest";
import { splitSpeechSamples, splitSpeechText } from "../scripts/local-models/speech-segments.mjs";

describe("语音有界取消检查点", () => {
  it.each(["这是一个很长的中文句子，用来验证所有文字都被保留。你好！", "A long English sentence with words and punctuation.", "😀".repeat(30), "", "纯连续文本".repeat(30)])("分句完整保留原文：%s", (text) => {
    const parts = splitSpeechText(text);
    expect(parts.join("")).toBe(text);
    expect(parts.every((part: string) => Array.from(part).length <= 12)).toBe(true);
  });

  it("短录音保留原段，长录音切分无丢样或重复采样", () => {
    const sampleRate = 100;
    const input = Float32Array.from({ length: 2500 }, (_, i) => Math.sin(i));
    const chunks = [...splitSpeechSamples(input, sampleRate)];
    expect(chunks.every((part: Float32Array) => part.length > 0 && part.length <= sampleRate * 8)).toBe(true);
    expect(chunks.flatMap((part: Float32Array) => [...part])).toEqual([...input]);
    const short = new Float32Array(200);
    expect([...splitSpeechSamples(short, sampleRate)]).toHaveLength(1);
  });
});
