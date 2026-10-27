/**
 * ui-helpers.ts — 连接状态 / 错误提示 / 模型加载
 *
 * 纯 store 操作，无 DOM 依赖。
 */

import { useStore } from '../stores';
import { chatFetch as lingxiFetch } from '../services/service-client';
import { captureSessionConnection, ownsSessionConnection } from '../services/session-connection-owner';
import { resolveServerConnection } from '../services/service-client';
import { errorBus } from '../../../../shared/error-bus.ts';
import { AppError } from '../../../../shared/errors.ts';

// ── 连接状态 ──

export function setStatus(key: string, connected: boolean, vars: Record<string, string | number> = {}): void {
  useStore.setState({ connected, statusKey: key, statusVars: vars });
}

// ── 错误显示 ──

export function showError(message: string): void {
  errorBus.report(new AppError('UNKNOWN', { message }));
}

// ── 模型加载 ──

export async function loadModels(options: { throwOnError?: boolean } = {}): Promise<void> {
  const owner = captureSessionConnection();
  try {
    const res = await lingxiFetch('/api/models');
    const data = await res.json();
    const { pendingNewSession } = useStore.getState();
    const activeModel = data.activeModel;
    if (!Array.isArray(data.models) || data.models.some((model: unknown) => !model || typeof model !== 'object'
      || typeof (model as {id?: unknown}).id !== 'string' || typeof (model as {provider?: unknown}).provider !== 'string')) {
      throw new Error('模型列表格式无效，已保留原列表');
    }
    let models = data.models.map((model: Record<string, unknown>) => ({ ...model,
      ...(model.defaultThinkingLevel == null ? { defaultThinkingLevel: undefined } : {}),
    }));

    // 非 pending 状态：用 session 实际绑定的 model 重写 isCurrent 标记
    // pending 状态：保持 API 返回的 isCurrent（跟 agent Chat model 走）
    if (activeModel && (!pendingNewSession || resolveServerConnection(useStore.getState())?.serverNodeKind === 'lingxi-service')) {
      models = models.map((m: any) => ({
        ...m,
        isCurrent: m.id === activeModel.id && m.provider === activeModel.provider,
      }));
    }

    const currentModelObj = models.find((m: any) => m.isCurrent);
    useStore.setState({
      models,
      currentModel: currentModelObj ? { id: currentModelObj.id, provider: currentModelObj.provider } : null,
    });
  } catch (error) {
    if (options.throwOnError) throw error;
    if (ownsSessionConnection(owner) && resolveServerConnection(useStore.getState())?.serverNodeKind === 'lingxi-service') {
      showError(error instanceof Error ? error.message : '模型列表读取失败');
    }
  }
}

