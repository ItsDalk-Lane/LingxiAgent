export const DESKTOP_INPUT_CORRELATION_TYPE = 'lingxi-desktop-input-correlation';

/** 同一客户端身份只允许对应一个本会话分支内的真实 user entry。 */
export function collectDesktopInputCorrelations(entries: any[], sessionId: string) {
  const users = new Map(entries.filter(entry => entry.type === 'message' && entry.message?.role === 'user').map(entry => [entry.id, entry]));
  const byEntry = new Map<string, Array<{ clientMessageId: string; snapshotVersion: number; sourceEntryId: string }>>();
  const byClient = new Map<string, Set<string>>();
  for (const entry of entries) {
    if (entry.type !== 'custom' || entry.customType !== DESKTOP_INPUT_CORRELATION_TYPE) continue;
    const data = entry.data;
    if (data?.schemaVersion !== 1 || data.sessionId !== sessionId || !users.has(data.sourceEntryId)
      || typeof data.clientMessageId !== 'string' || !data.clientMessageId
      || !Number.isSafeInteger(data.snapshotVersion) || data.snapshotVersion < 1) continue;
    const record = { clientMessageId: data.clientMessageId, snapshotVersion: data.snapshotVersion, sourceEntryId: data.sourceEntryId };
    const records = byEntry.get(data.sourceEntryId) || []; records.push(record); byEntry.set(data.sourceEntryId, records);
    const key = record.clientMessageId;
    const ids = byClient.get(key) || new Set<string>(); ids.add(data.sourceEntryId); byClient.set(key, ids);
  }
  const result = new Map<string, any>();
  for (const [id, records] of byEntry) {
    const identities = new Set(records.map(record => JSON.stringify([record.clientMessageId, record.snapshotVersion])));
    const record = records[0];
    if (identities.size !== 1 || byClient.get(record.clientMessageId)!.size !== 1) {
      result.set(id, { acceptanceDiagnostic: 'ambiguous' });
    } else result.set(id, record);
  }
  return result;
}
