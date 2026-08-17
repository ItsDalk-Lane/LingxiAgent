/**
 * ToolGroupBlock — 工具调用组，含展开/折叠
 */

import { memo, useState, useCallback, useEffect, useRef } from 'react';
import { Collapse } from '@/ui';
import styles from './Chat.module.css';
import { extractToolDetail } from '../../utils/message-parser';
import type { ToolDetail } from '../../utils/message-parser';
import { openInternalLink } from '../../utils/link-open';
import { isToolCallHiddenFromProcessUi } from '../../utils/tool-call-visibility';
import { getToolLabel, phaseForStatus, sessionToolTargetName, sessionToolTargetPath } from '../../utils/tool-label';
import { useStore } from '../../stores';
import { switchSession } from '../../stores/session-actions';
import { LinkContextMenu, type LinkContextMenuState } from '../shared/LinkContextMenu';
import { ChatResourceCard } from './ChatResourceCard';
import { TerminalPreview } from '../right-workspace/TerminalCard';
import { selectTerminalById } from '../../stores/terminal-slice';
import { subscribeChatCardNavigation } from '../../services/chat-card-navigation';
import { skillInvocationName } from '../../../../../shared/tool-outcome.ts';
import {
  asDeferredHistoryContent,
  useDeferredHistoryContent,
} from '../../hooks/use-deferred-history-content';

import type { ToolCall } from '../../stores/chat-types';

interface Props {
  tools: ToolCall[];
  collapsed: boolean;
  agentName?: string;
  skillPrompt?: string | null;
  sessionPath?: string;
}

export const ToolGroupBlock = memo(function ToolGroupBlock({
  tools: rawTools,
  collapsed: initialCollapsed,
  agentName = 'Lingxi',
  skillPrompt = null,
  sessionPath = '',
}: Props) {
  // 独立卡片或产物块承接状态的工具，不在工具组里重复显示。
  const tools = rawTools.filter(t => !isToolCallHiddenFromProcessUi(t));
  const [collapsed, setCollapsed] = useState(initialCollapsed);
  useEffect(() => {
    setCollapsed(initialCollapsed);
  }, [initialCollapsed]);
  const toggle = useCallback(() => setCollapsed(v => !v), []);

  if (tools.length === 0) return null;

  const allDone = tools.every(t => t.status ? t.status !== 'running' : t.done);
  const failCount = tools.filter(t => t.status === 'failed' || (!t.status && t.done && !t.success)).length;
  const isSingle = tools.length === 1;
  const standaloneTools = tools.filter((tool) => (
    tool.name === 'exec_command' || !!skillInvocationName({ toolName: tool.name, args: tool.args })
  ));
  const standardTools = tools.filter((tool) => (
    tool.name !== 'exec_command' && !skillInvocationName({ toolName: tool.name, args: tool.args })
  ));

  // 摘要标题
  const _t = window.t ?? ((p: string) => p);
  let summaryText = '';
  if (allDone) {
    if (failCount > 0) {
      summaryText = _t('toolGroup.countWithFail', { total: tools.length, fail: failCount });
    } else {
      summaryText = _t('toolGroup.count', { n: tools.length });
    }
  } else {
    const running = tools.filter(t => !t.done).length;
    summaryText = _t('toolGroup.running', { n: running });
  }

  return (
    <div className={`${styles.toolGroup}${isSingle ? ` ${styles.toolGroupSingle}` : ''}`}>
      {!isSingle && standardTools.length > 0 && (
        <div
          className={`${styles.toolGroupSummary}${allDone ? ` ${styles.toolGroupSummaryClickable}` : ''}`}
          onClick={allDone ? toggle : undefined}
        >
          <span className={styles.toolGroupTitle}>{summaryText}</span>
          {allDone && <span className={styles.toolGroupArrow}>{collapsed ? '›' : '‹'}</span>}
          {!allDone && (
            <span className={styles.toolDots} />
          )}
        </div>
      )}
      {standaloneTools.length > 0 && (
        <div className={`${styles.toolGroupContent} ${styles.toolGroupExecContent}`}>
          {standaloneTools.map((tool, i) => (
            <ToolIndicator
              key={tool.id || `${tool.name}-${i}`}
              tool={tool}
              agentName={agentName}
              skillPrompt={skillPrompt}
              sessionPath={sessionPath}
            />
          ))}
        </div>
      )}
      {isSingle && standardTools.length === 1 ? (
        <div className={styles.toolGroupContent}>
          <ToolIndicator tool={standardTools[0]} agentName={agentName} skillPrompt={skillPrompt} sessionPath={sessionPath} />
        </div>
      ) : standardTools.length > 0 ? (
        <Collapse open={!collapsed}>
          <div className={styles.toolGroupContent}>
            {standardTools.map((tool, i) => (
              <ToolIndicator
                key={tool.id || `${tool.name}-${i}`}
                tool={tool}
                agentName={agentName}
                skillPrompt={skillPrompt}
                sessionPath={sessionPath}
              />
            ))}
          </div>
        </Collapse>
      ) : null}
    </div>
  );
});

// ── ToolIndicator ──

function handleDetailClick(e: React.MouseEvent, detail: ToolDetail) {
  e.preventDefault();
  e.stopPropagation();
  if (!detail.href) return;
  void openInternalLink(detail.href, { origin: 'session' });
}

const ToolIndicator = memo(function ToolIndicator({
  tool,
  agentName,
  skillPrompt,
  sessionPath,
}: {
  tool: ToolCall;
  agentName: string;
  skillPrompt: string | null;
  sessionPath: string;
}) {
  if (tool.name === 'exec_command') {
    return <ExecCommandCard tool={tool} sessionPath={sessionPath} />;
  }
  if (skillInvocationName({ toolName: tool.name, args: tool.args })) {
    return <SkillInvocationCard tool={tool} skillPrompt={skillPrompt} sessionPath={sessionPath} />;
  }

  return <StandardToolIndicator tool={tool} agentName={agentName} />;
});

const StandardToolIndicator = memo(function StandardToolIndicator({ tool, agentName }: { tool: ToolCall; agentName: string }) {
  const [linkMenu, setLinkMenu] = useState<LinkContextMenuState | null>(null);

  // session 工具指向另一个会话，把它的名字显示出来并支持点过去。两个 selector 各返回
  // 字符串或 null，引用稳定，不会让每个工具行都因为 sessions 变动而重渲染。
  const isSessionTool = tool.name === 'session';
  const sessionTargetName = useStore(s => (isSessionTool ? sessionToolTargetName(s, tool.args) : null));
  const sessionTargetPath = useStore(s => (isSessionTool ? sessionToolTargetPath(s, tool.args) : null));

  const rawDetail = extractToolDetail(tool.name, tool.args);
  const detail = sessionTargetName ? { ...rawDetail, text: sessionTargetName } : rawDetail;
  const detailTitle = detail.title || detail.href;
  const status = tool.status || (tool.done ? (tool.success ? 'succeeded' : 'failed') : 'running');
  // 失败的工具要说失败：此前这里只传 done/running，失败的读文件会显示"翻完了 ✗"
  const label = getToolLabel(tool.name, phaseForStatus(status), agentName, tool.args);

  // 如果 args 里有 tag 类型信息（如 agent 名）
  const tag = tool.args?.agentId as string | undefined;

  return (
    <>
      <div className={styles.toolIndicator} data-tool={tool.name} data-done={String(tool.done)}>
        <span className={styles.toolDesc}>{label}</span>
        {detail.text && (
          sessionTargetPath ? (
            <span
              className={`${styles.toolDetail} ${styles.toolDetailLink}`}
              title={detailTitle || detail.text}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                void switchSession(sessionTargetPath);
              }}
            >
              {detail.text}
            </span>
          ) : detail.href ? (
            <span
              className={`${styles.toolDetail} ${styles.toolDetailLink}`}
              title={detailTitle}
              onClick={(e) => handleDetailClick(e, detail)}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                if (!detail.href) return;
                setLinkMenu({
                  href: detail.href,
                  context: { origin: 'session', label: detail.text },
                  position: { x: e.clientX, y: e.clientY },
                });
              }}
            >
              {detail.text}
            </span>
          ) : (
            <span className={styles.toolDetail} title={detailTitle}>{detail.text}</span>
          )
        )}
        {tool.error && (
          <span className={styles.toolDetail} title={tool.error}>{tool.error}</span>
        )}
        {tag && <span className={styles.toolTag}>{tag}</span>}
        {status !== 'running' ? (
          <span className={`${styles.toolStatus} ${status === 'succeeded' ? styles.toolStatusDone : styles.toolStatusFailed}`}>
            {status === 'succeeded' ? '✓' : status === 'failed' ? '✗' : '?'}
          </span>
        ) : (
          <span className={styles.toolDots} />
        )}
      </div>
      {linkMenu && (
        <LinkContextMenu
          state={linkMenu}
          onClose={() => setLinkMenu(null)}
        />
      )}
    </>
  );
});

function recordOf(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

const SkillInvocationCard = memo(function SkillInvocationCard({
  tool,
  skillPrompt,
  sessionPath,
}: {
  tool: ToolCall;
  skillPrompt: string | null;
  sessionPath: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const _t = window.t ?? ((path: string) => path);
  const skillName = skillInvocationName({ toolName: tool.name, args: tool.args }) || 'SKILL.md';
  const status = tool.status || (tool.done ? (tool.success ? 'succeeded' : 'failed') : 'running');
  const titleKey = status === 'running'
    ? 'toolGroup.skill.running'
    : status === 'failed'
      ? 'toolGroup.skill.failed'
      : 'toolGroup.skill.completed';
  const title = _t(titleKey, { name: skillName });
  const invocation = recordOf(tool.details?.skillInvocation);
  const deferred = asDeferredHistoryContent(invocation?.deferred);
  const loaded = useDeferredHistoryContent(sessionPath, deferred, expanded && !!sessionPath);
  const content = loaded.data?.content
    || (typeof invocation?.content === 'string' ? invocation.content : '');
  const truncated = invocation?.truncated === true;
  const invocationPrompt = skillPrompt?.trim() || _t('toolGroup.skill.promptUnavailable');
  const displayedContent = content
    ? `<skill_content name="${skillName}">\n${content}\n</skill_content>`
    : tool.error || _t(status === 'running' ? 'toolGroup.skill.contentPending' : 'toolGroup.skill.contentUnavailable');

  return (
    <div
      className={styles.skillInvocationCardHost}
      data-tool="read"
      data-tool-call-id={tool.id}
      data-skill-name={skillName}
    >
      <ChatResourceCard
        variant="task"
        compact
        className={styles.skillInvocationCard}
        icon={<span className={styles.skillInvocationIcon}>✣</span>}
        title={title}
        statusLabel={status === 'failed' ? '✗' : status === 'running' ? '…' : undefined}
        statusTone={status === 'failed' ? 'danger' : 'muted'}
        expandable
        expanded={expanded}
        onToggle={() => setExpanded((value) => !value)}
        ariaLabel={title}
      >
        <div className={styles.skillInvocationDetails}>
          <div className={styles.skillInvocationMetaRow}>
            <span>{_t('toolGroup.skill.skillLabel')}</span>
            <code>{skillName}</code>
          </div>
          <div className={styles.skillInvocationMetaRow}>
            <span>{_t('toolGroup.skill.paramsLabel')}</span>
            <pre>{invocationPrompt}</pre>
          </div>
          <pre className={styles.skillInvocationContent}>{displayedContent}</pre>
          {truncated && (
            <div className={styles.skillInvocationTruncated}>{_t('toolGroup.skill.truncated')}</div>
          )}
        </div>
      </ChatResourceCard>
    </div>
  );
});

const ExecCommandCard = memo(function ExecCommandCard({ tool, sessionPath }: { tool: ToolCall; sessionPath: string }) {
  const [expanded, setExpanded] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const _t = window.t ?? ((p: string) => p);
  const execDetails = recordOf(tool.details?.execCommand);
  const terminalId = typeof execDetails?.terminalId === 'string'
    ? execDetails.terminalId
    : typeof execDetails?.processId === 'string'
      ? execDetails.processId
      : null;
  // 终端注册在真正起 tty 的会话 key 下；子助手预览里渲染上下文是父会话 path，
  // 按 currentSessionPath 查永远查不到，必须按 terminalId 跨会话索引。
  const terminal = useStore(selectTerminalById(terminalId));
  const outputDeferred = asDeferredHistoryContent(tool.details?.outputDeferred);
  const loadedOutput = useDeferredHistoryContent(
    sessionPath,
    outputDeferred,
    expanded && !terminal && !!sessionPath,
  );
  const command = (
    (typeof tool.args?.cmd === 'string' && tool.args.cmd)
    || (typeof execDetails?.cmd === 'string' && execDetails.cmd)
    || (typeof execDetails?.renderedCommand === 'string' && execDetails.renderedCommand)
    || 'exec_command'
  );
  const renderedCommand = (
    (typeof execDetails?.renderedCommand === 'string' && execDetails.renderedCommand)
    || (typeof execDetails?.commandWithWorkdir === 'string' && execDetails.commandWithWorkdir)
    || command
  );
  const output = loadedOutput.data?.content
    || (typeof tool.details?.output === 'string' ? tool.details.output : '');
  const toolStatus = tool.status || (tool.done ? (tool.success ? 'succeeded' : 'failed') : 'running');
  const exitCode = terminal && Number.isFinite(terminal.exitCode) ? terminal.exitCode as number : null;
  // 后台终端启动后，exec_command 工具调用本身会先结束；卡片状态仍应跟随真实进程，
  // 不能在进程还运行时提前显示“已完成”。
  // stale（如 Lingxi 重启后 PTY 失联）是中性状态，不算成功也不算失败；
  // exited 但拿不到有效 exitCode 时退回工具调用自身的结果，不伪造失败。
  const status = terminal?.status === 'running'
    ? 'running'
    : terminal?.status === 'killed'
      ? 'failed'
      : terminal?.status === 'stale'
        ? 'stale'
        : terminal?.status === 'exited'
          ? exitCode === null ? toolStatus : exitCode === 0 ? 'succeeded' : 'failed'
          : toolStatus;

  useEffect(() => subscribeChatCardNavigation((request) => {
    if (request.kind !== 'terminal') return false;
    if (!request.ids.some((id) => id === tool.id || id === terminalId)) return false;
    setExpanded(true);
    window.requestAnimationFrame(() => {
      rootRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    });
    return true;
  }), [terminalId, tool.id]);

  return (
    <div
      ref={rootRef}
      className={styles.execCommandCardHost}
      data-tool="exec_command"
      data-tool-call-id={tool.id}
      data-terminal-id={terminalId || undefined}
    >
      <ChatResourceCard
        variant="task"
        compact
        className={styles.execCommandCard}
        icon={<span className={styles.execCommandIcon}>›_</span>}
        title={command}
        statusLabel={
          status === 'running'
            ? '…'
            : status === 'succeeded'
              ? '✓'
              : status === 'stale'
                ? _t('rightWorkspace.terminal.stale')
                : '✗'
        }
        statusTone={
          status === 'failed'
            ? 'danger'
            : status === 'succeeded'
              ? 'success'
              : status === 'stale'
                ? 'muted'
                : 'accent'
        }
        expandable
        expanded={expanded}
        onToggle={() => setExpanded((value) => !value)}
        ariaLabel={command}
      >
        <div className={styles.execCommandDetails}>
          <pre className={styles.execCommandLine}><span aria-hidden="true">$ </span>{renderedCommand}</pre>
          {terminal ? (
            <TerminalPreview terminal={terminal} />
          ) : (
            <pre className={styles.execCommandOutput}>{output || tool.error || ''}</pre>
          )}
        </div>
      </ChatResourceCard>
    </div>
  );
});
