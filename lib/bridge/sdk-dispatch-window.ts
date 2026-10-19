/** 同一可信管道的递增序号形成永久水位；只缓存近期结果，不累计终生墓碑。 */
import { createHash } from 'node:crypto';

export const SDK_RECENT_DISPATCHES = 256;
export function sdkCommandDigest(value: unknown): string {
  const canonical = JSON.stringify(value, (_key, item) => item && typeof item === 'object' && !Array.isArray(item)
    ? Object.fromEntries(Object.keys(item).sort().map(key => [key, item[key]])) : item);
  return createHash('sha256').update(canonical).digest('hex');
}
type Entry<T> = { digest: string; done: boolean; result?: T };
export class SdkDispatchWindow<T> {
  private watermark = 0;
  private recent = new Map<number, Entry<T>>();
  get size() { return this.recent.size; }
  begin(sequence: unknown, command: unknown): { fresh: boolean; entry: Entry<T> } {
    if (!Number.isInteger(sequence) || Number(sequence) < 1 || Number(sequence) > 0xffff_ffff) throw new Error('invalid_dispatch_sequence');
    const seq = Number(sequence), digest = sdkCommandDigest(command);
    const old = this.recent.get(seq);
    if (old) {
      if (old.digest !== digest) throw new Error('dispatch_sequence_conflict');
      return { fresh: false, entry: old };
    }
    if (seq <= this.watermark) throw new Error('retired_dispatch_sequence');
    this.watermark = seq;
    const entry: Entry<T> = { digest, done: false };
    this.recent.set(seq, entry); this.prune();
    return { fresh: true, entry };
  }
  complete(entry: Entry<T>, result: T) { entry.done = true; entry.result = result; this.prune(); }
  private prune() {
    for (const [seq, entry] of this.recent) {
      if (this.recent.size <= SDK_RECENT_DISPATCHES) break;
      if (entry.done) this.recent.delete(seq);
    }
  }
  clear() { this.recent.clear(); }
}
