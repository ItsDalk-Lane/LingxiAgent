/**
 * 消息流底部「正在做什么 + 已用多久」状态行。
 *
 * 文字是固定的「<助手名>正在工作中」——助手名按会话归属解析（同 ChatTranscript
 * 的口径），不随单步动作改口，避免每步闪一下。后面跟循环省略号，文字用一条
 * 浅色带扫过（对齐 DSH 同款状态行的流光），右侧秒表从这一轮开始独立计时。
 *
 * 读取口径：只认当前 session 最新一条 live assistant 快照里的「最后一个未收敛
 * 过程项」，用来决定这一行此刻该不该在（有在跑的工具 / 未封口思考 / 在流正文）。
 * 具体动作类别（read / grep / edit …）由 selectRunningActivity 保留，供将来按需
 * 切回逐步文案；当前只把「有没有在跑」当作显示条件。
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../../stores';
import {
  readLiveAssistantMessage,
  subscribeLiveAssistantMessage,
} from '../../stores/live-turn-store';
import type { ContentBlock, ToolCall } from '../../stores/chat-types';
import { isToolCallHiddenFromProcessUi } from '../../utils/tool-call-visibility';
import { runningStatusKey } from '../../utils/tool-label';
import { resolveAgentDisplayInfo } from '../../utils/agent-display';
import { formatRunningDuration, type DurationTranslate } from '../../utils/format-duration';
import styles from './RunningStatusLine.module.css';

export interface RunningActivity {
  /** `chat.running.*` 的叶子键。 */
  key: string;
  /** `chat.running.tool` 目录类模板要的工具名。 */
  tool?: string;
}

const THINKING: RunningActivity = { key: 'thinking' };
const WRITING: RunningActivity = { key: 'writing' };

function activityOfTools(tools: readonly ToolCall[]): RunningActivity | null {
  // 取最后一个而非第一个：并行工具里最新启动的那个才是此刻在转的。
  for (let i = tools.length - 1; i >= 0; i -= 1) {
    const tool = tools[i];
    if (isToolCallHiddenFromProcessUi(tool)) continue;
    // tool_start 建出来的在途调用只有 done:false、没有 status（见 tool-call-identity）。
    if (tool.status === 'running' || (tool.status === undefined && !tool.done)) {
      const key = runningStatusKey(tool.name);
      return key === 'tool' ? { key, tool: tool.name } : { key };
    }
  }
  return null;
}

/**
 * 从一个快照的 blocks 里取当前动作：最后一个在跑的工具优先，其次是未封口的
 * 思考段，再其次是一段还在流的正文（没有思考也没有工具，用户等的是回答本身）。
 */
export function selectRunningActivity(blocks: readonly ContentBlock[] | null | undefined): RunningActivity | null {
  if (!blocks?.length) return null;
  let thinking = false;
  let text = false;
  for (let i = blocks.length - 1; i >= 0; i -= 1) {
    const block = blocks[i];
    if (block.type === 'tool_group') {
      const active = activityOfTools(block.tools);
      if (active) return active;
      continue;
    }
    if (block.type === 'thinking' && !block.sealed) thinking = true;
    if (block.type === 'text') text = true;
  }
  if (thinking) return THINKING;
  return text ? WRITING : null;
}

/** 最新一条 assistant 消息 id：live 快照挂在它上面，与 ChatTranscript 同口径。 */
export function useLatestAssistantId(sessionPath: string): string {
  const items = useStore((state) => state.chatSessions[sessionPath]?.items);
  return useMemo(() => {
    if (!items) return '';
    for (let i = items.length - 1; i >= 0; i -= 1) {
      const item = items[i];
      if (item.type === 'message' && item.data.role === 'assistant') return item.data.id;
    }
    return '';
  }, [items]);
}

/**
 * 本会话归属助手的显示名。
 *
 * 与 ChatTranscript 同一套解析规则（会话 agentId → 全局 agentName 兜底），
 * 所以主窗口、快捷聊天窗、Bridge 面板三处对同一会话叫同一个名字。
 */
export function useSessionAgentName(sessionPath: string): string {
  const sessionAgentId = useStore((state) => (
    state.sessions.find((item) => item.path === sessionPath)?.agentId ?? null
  ));
  const agents = useStore((state) => state.agents);
  const fallbackName = useStore((state) => state.agentName) || 'Lingxi';
  const fallbackYuan = useStore((state) => state.agentYuan) || 'lingxi';
  return useMemo(() => resolveAgentDisplayInfo({
    id: sessionAgentId,
    agents,
    fallbackAgentName: fallbackName,
    fallbackAgentYuan: fallbackYuan,
  }).displayName, [agents, fallbackName, fallbackYuan, sessionAgentId]);
}

/**
 * 当前动作订阅：live-turn-store 每次 publish 都通知，但只有「动作身份」真的换了
 * 才 setState（同一动作流式追加 token 不重渲染这一行）。
 */
function useRunningActivity(sessionPath: string, messageId: string): RunningActivity | null {
  const [identity, setIdentity] = useState<{ key: string; tool?: string } | null>(null);
  useEffect(() => {
    if (!messageId) {
      setIdentity(null);
      return;
    }
    const sync = () => {
      const next = selectRunningActivity(readLiveAssistantMessage(sessionPath, messageId)?.blocks);
      setIdentity(current => (
        current?.key === next?.key && current?.tool === next?.tool
          ? current
          : next ? { key: next.key, ...(next.tool ? { tool: next.tool } : {}) } : null
      ));
    };
    sync();
    return subscribeLiveAssistantMessage(sessionPath, messageId, sync);
  }, [messageId, sessionPath]);
  return identity;
}

interface Props {
  sessionPath: string;
  /** 本地「等待助手」态：发送即置位，此时还没有 live 消息。 */
  pending: boolean;
  /** 知识库检索态：工具动作发生前的第一段等待。 */
  knowledgeRetrieving: boolean;
  /** 本会话助手的显示名；缺省按会话解析，再兜底全局助手名。 */
  agentName?: string;
}

export function RunningStatusLine({ sessionPath, pending, knowledgeRetrieving, agentName }: Props) {
  // 起表时间跨渲染固定：本轮状态行出现的那一刻。pending 与 streaming 之间不重置，
  // 否则「发送 → 首个事件」这段等待会被抹掉，计数看起来会归零。
  const originRef = useRef(Date.now());
  const latestAssistantId = useLatestAssistantId(sessionPath);
  const activity = useRunningActivity(sessionPath, latestAssistantId);
  const resolvedAgentName = useSessionAgentName(sessionPath);

  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    const tick = () => setElapsed(Date.now() - originRef.current);
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, []);

  const t = window.t ?? ((key: string) => key);
  // window.t 在类型上是「可能不存在」的，duration 模板要一个确定可调用的翻译函数。
  const translate: DurationTranslate = (key, vars) => t(key, vars);
  // 文案固定成「<助手名>正在工作中」：这一行的口径是「谁在干活」，不是「干到哪一步」，
  // 所以不随 activity.key 换词（knowledge 检索期也照同一句）。
  const label = t('chat.running.working', { name: (agentName || resolvedAgentName).trim() });

  return (
    <div
      className={styles.status}
      role="status"
      aria-live="polite"
      data-running-status={activity?.key ?? 'idle'}
      data-pending={pending ? 'true' : undefined}
      data-knowledge-retrieving={knowledgeRetrieving ? 'true' : undefined}
    >
      <span className={styles.label}>{label}</span>
      <span className={styles.dots} aria-hidden="true" />
      {/* 秒表每秒都在变，别让读屏器跟着念：状态本身由上面的 aria-live 播报。 */}
      <span className={styles.timer} aria-hidden="true" data-running-timer="">
        {formatRunningDuration(elapsed, translate)}
      </span>
    </div>
  );
}
