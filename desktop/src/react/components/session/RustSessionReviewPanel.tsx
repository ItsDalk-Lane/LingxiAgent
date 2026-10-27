import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useStore } from '../../stores';
import { grantRustMcpSessionCapability } from '../../services/mcp-session-permissions';
import { captureSessionConnection, ownsSessionConnection, sessionConnectionRevision, sessionOwnerFetch,
  subscribeSessionConnection, sessionSelectionRevision, type SessionConnectionOwner } from '../../services/session-connection-owner';

interface Approval { approvalId: string; runId: string; summary: string; target: string; capability?: string | null }
interface Receipt { status: string; error?: string; checkpoint: string; persistenceError?: string }
interface View { selection: number; owner: SessionConnectionOwner; sessionId: string; approvals: Approval[]; receipt: Receipt | null }

/** 显示生产批准服务的真实请求，不把模型提供的风险声明当作用户批准。 */
export default function RustSessionReviewPanel() {
  const sessionId = useStore(state => state.currentSessionId);
  const revision = useSyncExternalStore(subscribeSessionConnection, sessionConnectionRevision);
  const selection = useSyncExternalStore(subscribeSessionConnection, sessionSelectionRevision);
  const operation = useRef(0);
  const [view, setView] = useState<View | null>(null);
  const [error, setError] = useState<{ owner: SessionConnectionOwner; sessionId: string; selection: number; text: string } | null>(null);
  const [busy, setBusy] = useState<{ id: string; sessionId: string; selection: number } | null>(null);
  const visible = view && view.sessionId === sessionId && view.selection === selection && ownsSessionConnection(view.owner) ? view : null;
  const current = captureSessionConnection();
  const rust = current?.connection.serverNodeKind === 'lingxi-service';
  useEffect(() => {
    const owner = captureSessionConnection();
    if (!sessionId || !owner || owner.connection.serverNodeKind !== 'lingxi-service') return;
    const targetSelection = sessionSelectionRevision();
    const currentTarget = () => ownsSessionConnection(owner) && useStore.getState().currentSessionId === sessionId && sessionSelectionRevision() === targetSelection;
    let disposed = false;
    let waiting = false;
    setError(null); setBusy(null);
    const refresh = async () => {
      if (waiting || disposed || !currentTarget()) return;
      waiting = true;
      try {
        const base = `/lingxi/v1/sessions/${encodeURIComponent(sessionId)}`;
        const [approvalsResponse, receiptResponse] = await Promise.all([
          sessionOwnerFetch(owner, `${base}/approvals`), sessionOwnerFetch(owner, `${base}/deferred-rewind`),
        ]);
        const approvals: unknown = await approvalsResponse.json();
        const receipt: unknown = await receiptResponse.json();
        if (!approvals || typeof approvals !== 'object' || !Array.isArray((approvals as { approvals?: unknown }).approvals)) throw new Error('确认请求格式无效');
        const items = (approvals as { approvals: unknown[] }).approvals;
        if (items.some(item => !item || typeof item !== 'object'
          || typeof (item as Approval).approvalId !== 'string' || typeof (item as Approval).runId !== 'string'
          || typeof (item as Approval).summary !== 'string' || typeof (item as Approval).target !== 'string')) throw new Error('确认请求格式无效');
        if (!receipt || typeof receipt !== 'object' || !('deferredRewind' in receipt)) throw new Error('回退结果格式无效');
        const result = (receipt as { deferredRewind: Receipt | null }).deferredRewind;
        if (result && (typeof result.status !== 'string' || typeof result.checkpoint !== 'string')) throw new Error('回退结果格式无效');
        if (!disposed && currentTarget()) { setView({ owner, selection: targetSelection, sessionId, approvals: items as Approval[], receipt: result }); setError(null); }
      } catch (failure) {
        if (!disposed && currentTarget()) setError({ owner, selection: targetSelection, sessionId, text: failure instanceof Error ? failure.message : String(failure) });
      } finally { waiting = false; }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 600);
    return () => { disposed = true; clearInterval(timer); };
  }, [sessionId, revision, selection]);
  const answer = async (approvalId: string, action: 'confirmed' | 'rejected', remember = false) => {
    if (!visible || !ownsSessionConnection(visible.owner)
      || useStore.getState().currentSessionId !== visible.sessionId
      || sessionSelectionRevision() !== visible.selection
      || (busy && busy.sessionId === visible.sessionId && busy.selection === visible.selection)) return;
    const { owner, sessionId: target, selection: targetSelection } = visible;
    const generation = ++operation.current;
    const currentOperation = () => operation.current === generation && ownsSessionConnection(owner)
      && useStore.getState().currentSessionId === target && sessionSelectionRevision() === targetSelection;
    setBusy({ id: approvalId, sessionId: target, selection: targetSelection }); setError(null);
    let grantSaved = false;
    try {
      if (remember) {
        const approval = visible.approvals.find(item => item.approvalId === approvalId);
        if (!approval?.target.startsWith('tool:mcp:') || typeof approval.capability !== 'string' || !approval.capability) throw new Error('真实MCP能力尚未就绪');
        await grantRustMcpSessionCapability(owner, { sessionId: target, targetId: approval.target, capability: approval.capability });
        grantSaved = true;
        if (!currentOperation()) return;
      }
      const response = await sessionOwnerFetch(owner, `/lingxi/v1/sessions/${encodeURIComponent(target)}/approvals/${encodeURIComponent(approvalId)}`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ action }),
      });
      const data = await response.json();
      if (!data || data.ok !== true || !['confirmed', 'rejected'].includes(data.decision)) throw new Error('确认结果格式无效');
      if (currentOperation()) setView(previous => previous?.owner === owner ? { ...previous, approvals: previous.approvals.filter(item => item.approvalId !== approvalId) } : previous);
    } catch (failure) { if (currentOperation()) setError({ owner, selection: targetSelection, sessionId: target, text: `${grantSaved ? '本会话授权已生效；本次操作确认失败：' : ''}${failure instanceof Error ? failure.message : String(failure)}` }); }
    finally { if (currentOperation()) setBusy(null); }
  };
  if (!rust || !sessionId) return null;
  const visibleError = error && error.sessionId === sessionId && error.selection === selection && ownsSessionConnection(error.owner) ? error.text : null;
  return <section aria-busy={busy?.sessionId === sessionId && busy.selection === selection} aria-label="会话操作确认" data-testid="rust-session-review">
    {visibleError && <p role="alert">读取或执行会话操作失败：{visibleError}</p>}
    {visible?.approvals.map(approval => <div key={approval.approvalId} data-approval-id={approval.approvalId}>
      <p>{approval.summary}</p>
      <button disabled={busy?.sessionId === sessionId && busy.selection === selection} onClick={() => void answer(approval.approvalId, 'confirmed')}>确认执行</button>
      <button disabled={busy?.sessionId === sessionId && busy.selection === selection} onClick={() => void answer(approval.approvalId, 'rejected')}>拒绝</button>
      {approval.target.startsWith('tool:mcp:') && typeof approval.capability === 'string' && approval.capability && <button data-testid="rust-mcp-approve-session" disabled={busy?.sessionId === sessionId && busy.selection === selection} onClick={() => void answer(approval.approvalId, 'confirmed', true)}>本会话此工具不再询问</button>}
    </div>)}
    {visible?.receipt && <p role={['failed', 'unconfirmed_after_restart'].includes(visible.receipt.status) ? 'alert' : 'status'}>
      {visible.receipt.status === 'scheduled' ? '已确认，等待本轮结束后回退。'
        : visible.receipt.status === 'completed' ? `已回退到检查点「${visible.receipt.checkpoint}」。`
          : visible.receipt.status === 'failed' ? `回退失败：${visible.receipt.error ?? '请查看操作记录'}`
            : '服务已重启，无法确认上次排队回退的最终结果，请先核对会话记录。'}
      {visible.receipt.persistenceError && `结果记录保存失败：${visible.receipt.persistenceError}`}
    </p>}
  </section>;
}
