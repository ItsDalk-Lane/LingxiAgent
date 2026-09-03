// 所有片段拼接后仍是原文；优先沿标点切分，长句最多十二个字符，给取消留出检查点。
export function splitSpeechText(text) {
  const chars = Array.from(text), parts = [];
  let start = 0;
  while (start < chars.length) {
    let end = Math.min(start + 12, chars.length);
    if (end < chars.length) {
      for (let i = end - 1; i > start; i--) {
        if (/[，。！？、；：,.!?;:\s]/u.test(chars[i])) { end = i + 1; break; }
      }
    }
    parts.push(chars.slice(start, end).join('')); start = end;
  }
  return parts;
}

// 优先在末尾一秒中的低能量处切分；不跳过任何采样点，避免整段录音阻塞取消。
export function* splitSpeechSamples(samples, sampleRate) {
  const limit = Math.round(sampleRate * 8), window = Math.max(1, Math.round(sampleRate * 0.02));
  let start = 0;
  while (start < samples.length) {
    let end = Math.min(start + limit, samples.length);
    if (end < samples.length) {
      let lowest = Infinity;
      for (let at = end - sampleRate; at + window <= start + limit; at += window) {
        let energy = 0;
        for (let i = at; i < at + window; i++) energy += samples[i] * samples[i];
        if (energy < lowest) { lowest = energy; end = at + Math.floor(window / 2); }
      }
    }
    yield samples.subarray(start, end); start = end;
  }
}
