/**
 * host-capability-error.ts — 宿主能力受限的显式错误类型
 *
 * 红线「禁止静默降级」：受限能力要么抛 HostCapabilityError（带宿主与理由），
 * 要么显式 no-op 并在 DesktopHost.capabilityReasons 登记理由。
 */

export const HOST_CAPABILITY_UNAVAILABLE = 'HOST_CAPABILITY_UNAVAILABLE' as const;

export class HostCapabilityError extends Error {
  readonly code = HOST_CAPABILITY_UNAVAILABLE;

  constructor(
    readonly capability: string,
    readonly hostKind: string,
    readonly reason: string,
  ) {
    super(`宿主能力不可用（${hostKind}）：${capability} — ${reason}`);
    this.name = 'HostCapabilityError';
  }
}

export function isHostCapabilityError(error: unknown): error is HostCapabilityError {
  return error instanceof HostCapabilityError
    || (typeof error === 'object' && error !== null
      && (error as { code?: unknown }).code === HOST_CAPABILITY_UNAVAILABLE);
}
