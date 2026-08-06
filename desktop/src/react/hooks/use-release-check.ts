import { useCallback, useEffect, useState } from 'react';
import type { ReleaseCheckResult } from '../types';

/**
 * GitHub Release 版本检查 hook——Settings > About「检查更新」主检测源。
 *
 * 为什么不是 OTA 列车通道（useTrainUpdateState）：OTA 依赖签名过的 channel
 * manifest（LINGXI_ARTIFACT_CHANNEL_BASE_URL），正式构建里从未配置，导致
 * ota-state.json 残留一条永不清除的"检查更新失败"。这里直接查 GitHub
 * Releases，不依赖签名、不依赖环境变量，每次检查都是即时网络结果，失败
 * 也只停留在本次会话内存里，不会落盘永久残留。
 *
 * 界面契约：
 * - 挂载时自动触发一次检查（与 useTrainUpdateState 的 mount 行为一致），
 *   让用户打开 About 页就能看到当前是不是最新。
 * - `status` 四态：idle（尚未检查）/ checking（请求中）/ latest（已是最新）/
 *   available（发现新版本，带 releaseUrl 供"下载"按钮使用）/ error（失败，
 *   带可点重试）。
 * - `lastCheckedAt` 只在检查完成后填，latest/available/error 态都有。
 */

export type ReleaseCheckStatus = 'idle' | 'checking' | 'latest' | 'available' | 'error';

export interface UseReleaseCheckResult {
  status: ReleaseCheckStatus;
  /** 最新 release 版本号（去 v 前缀），latest/available 态有值。 */
  latestVersion?: string;
  /** release 页面地址，available 态作为"下载最新版本"按钮目标。 */
  releaseUrl?: string | null;
  /** error 态的简短说明（非内部细节）。 */
  error?: string;
  /** 最近一次检查完成时间（ISO），用于"上次检查 {time}"文案。 */
  lastCheckedAt: string | null;
  /** 触发一轮手动检查。 */
  checkNow(): Promise<void>;
}

async function runCheck(): Promise<ReleaseCheckResult | null> {
  try {
    return (await window.hana?.releaseCheckLatest?.()) ?? null;
  } catch {
    return null;
  }
}

export function useReleaseCheck(): UseReleaseCheckResult {
  const [status, setStatus] = useState<ReleaseCheckStatus>('idle');
  const [latestVersion, setLatestVersion] = useState<string | undefined>(undefined);
  const [releaseUrl, setReleaseUrl] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | undefined>(undefined);
  const [lastCheckedAt, setLastCheckedAt] = useState<string | null>(null);

  const applyResult = useCallback((result: ReleaseCheckResult | null) => {
    setLastCheckedAt(new Date().toISOString());
    if (!result) {
      setStatus('error');
      setError('unavailable');
      return;
    }
    setStatus(result.status);
    setLatestVersion(result.latestVersion);
    setReleaseUrl(result.releaseUrl);
    setError(result.error);
  }, []);

  const checkNow = useCallback(async () => {
    setStatus('checking');
    const result = await runCheck();
    applyResult(result);
  }, [applyResult]);

  // 挂载自动检查一次——用户打开 About 页就能看到是否最新，无需手动点。
  useEffect(() => {
    let alive = true;
    (async () => {
      setStatus('checking');
      const result = await runCheck();
      if (!alive) return;
      applyResult(result);
    })();
    return () => { alive = false; };
  }, [applyResult]);

  return { status, latestVersion, releaseUrl, error, lastCheckedAt, checkNow };
}
