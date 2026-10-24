import { lingxiFetch } from '../services/service-client';

export type UserEditCheckpointReason = 'edit-start' | 'autosave-interval';

export async function requestUserEditCheckpoint(
  filePath: string,
  reason: UserEditCheckpointReason,
): Promise<void> {
  await lingxiFetch('/api/checkpoints/user-edit', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ filePath, reason }),
  });
}
