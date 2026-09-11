/** 工具行保持发生顺序，整轮收纳由外层 ProcessFold 承担。 */
import { memo, useEffect, useRef, useState } from 'react';
import { Overlay } from '@/ui';
import type { ToolCall } from '../../stores/chat-types';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import { isSyntheticToolPresentation, safeToolArguments, parseToolSearchOutput, toolPatchStats } from '../../../../../shared/tool-presentation.ts';
import { skillInvocationName } from '../../../../../shared/tool-outcome.ts';
import { useStore } from '../../stores';
import { selectTerminalById } from '../../stores/terminal-slice';
import { switchSession } from '../../stores/session-actions';
import { subscribeChatCardNavigation } from '../../services/chat-card-navigation';
import { asDeferredHistoryContent, useDeferredHistoryContent } from '../../hooks/use-deferred-history-content';
import { extractToolDetail } from '../../utils/message-parser';
import { openInternalLink, resolveLinkTarget } from '../../utils/link-open';
import { isToolCallHiddenFromProcessUi } from '../../utils/tool-call-visibility';
import { getToolLabel, phaseForStatus, sessionToolTargetName, sessionToolTargetPath } from '../../utils/tool-label';
import { knowledgeResearchStopNote } from '../../utils/knowledge-research-status';
import { TerminalPreview } from '../right-workspace/TerminalCard';
import { LinkContextMenu, type LinkContextMenuState } from '../shared/LinkContextMenu';
import { SearchActivity } from './SearchActivity';
import { ActivityIcon, ActivityLines, CopyActivity, activityText as t } from './MessageActivity';
import styles from './MessageActivity.module.css';
import chatStyles from './Chat.module.css';

interface Props {
  tools: ToolCall[]; collapsed: boolean; agentName?: string; skillPrompt?: string | null;
  sessionPath?: string; knowledgeResearch?: KnowledgeRetrievalStats['research'];
}

export const ToolGroupBlock = memo(function ToolGroupBlock({ tools: rawTools, agentName = 'Lingxi', skillPrompt = null, sessionPath = '', knowledgeResearch }: Props) {
  const translate = window.t ?? ((key: string) => key);
  const tools = rawTools.filter(tool => !isToolCallHiddenFromProcessUi(tool)).map(tool => {
    if (!knowledgeResearch || !tool.done) return tool;
    if (tool.name === 'knowledge_research_worker' && (tool.args?.workerStatus === 'cancelled' || tool.resultNote === translate('chat.knowledgeResearchCancelled'))) {
      const note = knowledgeResearchStopNote(tool.args?.stopReason ?? knowledgeResearch.stopReason, translate);
      return note ? { ...tool, resultNote: note } : tool;
    }
    if (tool.name === 'knowledge_research_round' && !tool.args?.roundStatus && tool.args?.round === knowledgeResearch.rounds && knowledgeResearch.status !== 'completed' && tool.success) return { ...tool, status: 'unknown' as const, success: false };
    return tool;
  });
  if (!tools.length) return null;
  return <div className={chatStyles.toolGroup}>{tools.map((tool, index) => <ToolActivity key={tool.id || `${tool.name}-${index}`} tool={tool} agentName={agentName} skillPrompt={skillPrompt} sessionPath={sessionPath} />)}</div>;
});

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
}
function string(value: unknown): string { return typeof value === 'string' ? value : ''; }
function commandOf(tool: ToolCall): string {
  const exec = record(tool.details?.execCommand);
  return string(tool.args?.cmd) || string(tool.args?.command) || string(exec.cmd) || string(exec.renderedCommand) || string(tool.args?.chars);
}
const terminalNames = new Set(['exec_command', 'bash', 'terminal', 'write_stdin']);

const ToolActivity = memo(function ToolActivity({ tool, agentName, skillPrompt, sessionPath }: { tool: ToolCall; agentName: string; skillPrompt: string | null; sessionPath: string }) {
  const [expanded, setExpanded] = useState(false);
  const [linkMenu, setLinkMenu] = useState<LinkContextMenuState | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const exec = record(tool.details?.execCommand);
  const terminalId = string(exec.terminalId) || string(exec.processId) || null;
  const terminal = useStore(selectTerminalById(terminalId));
  const targetName = useStore(state => tool.name === 'session' ? sessionToolTargetName(state, tool.args) : null);
  const targetPath = useStore(state => tool.name === 'session' ? sessionToolTargetPath(state, tool.args) : null);
  const skillName = skillInvocationName({ toolName: tool.name, args: tool.args });
  const research = isSyntheticToolPresentation(tool.name);
  const toolStatus = tool.status || (tool.done ? tool.success ? 'succeeded' : 'failed' : 'running');
  const exitCode = terminal && Number.isFinite(terminal.exitCode) ? terminal.exitCode : null;
  const status = terminal?.status === 'running' ? 'running' : terminal?.status === 'killed' ? 'failed' : terminal?.status === 'stale' ? 'stale' : terminal?.status === 'exited' && exitCode !== null ? exitCode === 0 ? 'succeeded' : 'failed' : toolStatus;
  const detail = research ? { text: tool.name === 'knowledge_research_worker' ? string(tool.args?.label).slice(0, 100) : '', href: undefined, title: undefined } : extractToolDetail(tool.name, record(safeToolArguments(tool.args)));
  const recordedPath = string(record(tool.details?.read).path) || string(record(tool.details?.fileChange).path);
  const candidateHref = recordedPath || detail.href;
  const detailHref = candidateHref && resolveLinkTarget(candidateHref).kind !== 'external' ? candidateHref : undefined;
  const command = commandOf(tool);
  const summary = tool.error || (skillName ? skillName : targetName || (terminalNames.has(tool.name) ? string(tool.args?.description) || string(exec.description) || command.split('\n')[0] : detail.text));
  const label = research ? getToolLabel(tool.name, phaseForStatus(toolStatus), agentName, tool.args).replace(/^[^\p{L}\p{N}]+/u, '') : t(`labels.${skillName ? 'skill' : terminalNames.has(tool.name) ? 'terminal' : ['read', 'write', 'edit', 'grep', 'find', 'ls', 'session', 'web_search', 'web_fetch'].includes(tool.name) ? tool.name : 'tool'}`);
  const displayLabel = label === t('labels.tool') ? tool.name : label;
  const change = record(tool.details?.fileChange);
  const counts = typeof change.added === 'number' && typeof change.removed === 'number'
    ? { added: change.added, removed: change.removed }
    : !change.patchDeferred && !change.truncated ? toolPatchStats(string(change.patch)) || { added: 0, removed: 0 } : { added: 0, removed: 0 };
  useEffect(() => subscribeChatCardNavigation(request => {
    if (request.kind !== 'terminal' || !request.ids.some(id => id === tool.id || id === terminalId)) return false;
    setExpanded(true);
    window.requestAnimationFrame(() => root.current?.scrollIntoView({ behavior: 'smooth', block: 'center' }));
    return true;
  }), [terminalId, tool.id]);
  return <div ref={root} className={styles.activity} data-tool={tool.name} data-tool-call-id={tool.id} data-terminal-id={terminalId || undefined} data-skill-name={skillName || undefined} data-status={status} data-done={String(tool.done)}>
    <div role="button" tabIndex={0} aria-expanded={expanded} aria-label={`${displayLabel} · ${summary}`} className={`${styles.row} ${status === 'failed' ? styles.failed : ''}`} onClick={() => setExpanded(value => !value)} onKeyDown={event => {
      if (event.target !== event.currentTarget) return;
      if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); setExpanded(value => !value); }
    }}>
      <ActivityIcon kind={skillName ? 'skill' : tool.name} />
      <span className={styles.label}>{displayLabel}</span>
      {(summary || tool.resultNote) && <span className={styles.separator} aria-hidden="true">·</span>}
      {summary && <span className={styles.summary} title={detail.title || command || summary}>
        {(targetPath || detailHref) && !tool.error && !skillName ? <a className={styles.link} href={targetPath || detailHref} onClick={event => { event.preventDefault(); event.stopPropagation(); if (targetPath) void switchSession(targetPath); else if (detailHref) void openInternalLink(detailHref, { origin: 'session' }); }} onContextMenu={event => {
          if (!detailHref) return;
          event.preventDefault(); event.stopPropagation();
          setLinkMenu({ href: detailHref, context: { origin: 'session', label: summary }, position: { x: event.clientX, y: event.clientY } });
        }}>{summary}</a> : summary}
      </span>}
      {status === 'succeeded' && (counts.added > 0 || counts.removed > 0) && <span className={styles.counts}>+{counts.added} −{counts.removed}</span>}
      {tool.resultNote && <span className={styles.summary}>{tool.resultNote}</span>}
      {status !== 'succeeded' && <span className={styles.status}>{status === 'stale' ? (window.t?.('rightWorkspace.terminal.stale') || t('unknown')) : t(status)}</span>}
    </div>
    {expanded && <ToolActivityDetails tool={tool} skillName={skillName} skillPrompt={skillPrompt} sessionPath={sessionPath} terminal={terminal} status={status} />}
    {linkMenu && <LinkContextMenu state={linkMenu} onClose={() => setLinkMenu(null)} />}
  </div>;
});

function ToolActivityDetails({ tool, skillName, skillPrompt, sessionPath, terminal, status }: {
  tool: ToolCall; skillName: string | null; skillPrompt: string | null; sessionPath: string;
  terminal: ReturnType<ReturnType<typeof selectTerminalById>>; status: string;
}) {
  const [view, setView] = useState(false);
  const [terminalContent, setTerminalContent] = useState({ content: '', truncated: false });
  const details = tool.details || {};
  const research = isSyntheticToolPresentation(tool.name);
  const invocation = record(details.skillInvocation);
  const change = record(details.fileChange);
  const read = record(details.read);
  const search = record(details.search);
  const load = (value: unknown) => asDeferredHistoryContent(value);
  const inputDeferred = load(details.inputDeferred);
  const outputDeferred = load(details.outputDeferred);
  const patchDeferred = load(change.patchDeferred);
  const contentDeferred = load(change.contentDeferred);
  const skillDeferred = load(invocation.deferred);
  const inputLoad = useDeferredHistoryContent(sessionPath, inputDeferred, !research && !!sessionPath);
  const outputLoad = useDeferredHistoryContent(sessionPath, outputDeferred, !research && !!sessionPath);
  const patchLoad = useDeferredHistoryContent(sessionPath, patchDeferred, !research && !!sessionPath);
  const contentLoad = useDeferredHistoryContent(sessionPath, contentDeferred, !research && !!sessionPath);
  const skillLoad = useDeferredHistoryContent(sessionPath, skillDeferred, !!skillName && !!sessionPath);
  const loads = [inputLoad, outputLoad, patchLoad, contentLoad, skillLoad];
  const loading = loads.some(item => item.loading);
  const loadError = loads.some(item => item.error);
  const missingSession = !sessionPath && [inputDeferred, outputDeferred, patchDeferred, contentDeferred, skillDeferred].some(Boolean);
  const input = inputLoad.data?.content ?? string(details.input);
  const output = outputLoad.data?.content ?? string(details.output);
  let fullInputCommand = '';
  try {
    const args = record(JSON.parse(input));
    fullInputCommand = string(args.cmd) || string(args.command) || string(args.chars);
  } catch {
    // 存量调用可能没有结构化参数，退回该调用保存的命令。
  }
  const patch = patchLoad.data?.content ?? string(change.patch);
  const content = contentLoad.data?.content ?? string(change.content);
  const hasContent = typeof contentLoad.data?.content === 'string' || typeof change.content === 'string';
  const skillContent = skillLoad.data?.content ?? string(invocation.content);
  // SDK 的范围通知属于读取状态，不是文件正文；复制与原始详情仍保留通知。
  const readNoticeMatch = read.truncated === true ? /(?:\r?\n){0,2}\[(?:Showing lines [^\]\r\n]*|\d+ more lines in file\.[^\]\r\n]*)\]\s*$/.exec(output) : null;
  const readOutput = readNoticeMatch ? output.slice(0, readNoticeMatch.index) : output;
  const readNotice = readNoticeMatch?.[0].trim();
  const isTerminal = terminalNames.has(tool.name);
  const path = string(read.path) || string(change.path) || string(tool.args?.path) || string(tool.args?.file_path);
  const title = skillName || path || (isTerminal ? string(record(details.execCommand).workdir) || string(tool.args?.workdir) : tool.name);
  const truncated = Boolean(details.inputTruncated || details.outputTruncated || read.truncated || search.truncated || change.truncated || invocation.truncated);
  const available = !loading && !loadError && !missingSession;
  const copy = research ? tool.resultNote || '' : skillName ? skillContent : terminal ? terminalContent.content || output : patch || content || output;
  const loadedSearch = !Array.isArray(search.files) && ['grep', 'find', 'ls'].includes(string(search.kind)) && output ? parseToolSearchOutput(search.kind as 'grep' | 'find' | 'ls', output) : null;
  const files = Array.isArray(search.files) ? search.files.map(record) : (loadedSearch?.files || []).map(record);
  const changeVisible = Object.keys(change).length > 0 && status !== 'failed';
  const counts = typeof change.added === 'number' && typeof change.removed === 'number' ? { added: change.added, removed: change.removed } : !change.truncated && (!patchDeferred || patchLoad.data) && patch ? toolPatchStats(patch) || null : null;
  const empty = status === 'running' ? t('pending') : t('unavailable');
  return <>
    <div className={styles.panel}>
      <div className={styles.header}><span className={styles.title}>{title}</span>{Object.keys(read).length > 0 && <span className={styles.meta}>{t('readLines', { shown: typeof read.displayedLines === 'number' ? read.displayedLines : readOutput ? readOutput.split('\n').length : 0, total: typeof read.totalLines === 'number' ? read.totalLines : '—' })} {string(read.language)}</span>}<CopyActivity content={copy} disabled={!available || !copy} labelKey={terminal && terminalContent.truncated ? 'copyRetained' : 'copy'} /></div>
      <div className={styles.body}>
        {loading && <div className={styles.notice}>{t('loading')}</div>}
        {(loadError || missingSession) && <div className={styles.failed}>{t(loadError ? 'loadFailed' : 'unavailable')}</div>}
        {truncated && <div className={styles.notice}>{t('truncated')}</div>}
        {research ? <div className={styles.notice}>{tool.resultNote || t('researchPrivate')}</div>
          : skillName ? <><h4 className={styles.section}>{window.t?.('toolGroup.skill.paramsLabel')}</h4><pre className={styles.pre}>{skillPrompt?.trim() || window.t?.('toolGroup.skill.promptUnavailable')}</pre><pre className={styles.pre}>{skillContent ? `<skill_content name="${skillName}">\n${skillContent}\n</skill_content>` : empty}</pre></>
          : isTerminal ? <><pre className={styles.pre}>$ {fullInputCommand || string(record(details.execCommand).renderedCommand) || commandOf(tool)}</pre><div className={styles.notice}>{t(status === 'stale' ? 'unknown' : status)}</div>{terminal && terminalContent.truncated && <div className={styles.notice}>{t('terminalRetained')}</div>}<div className={styles.terminal}>{terminal ? <TerminalPreview terminal={terminal} onContentChange={setTerminalContent} /> : <pre className={styles.pre}>{output || tool.error || empty}</pre>}</div></>
          : Object.keys(read).length > 0 ? <>{readOutput ? <ActivityLines content={readOutput} startLine={Number(read.startLine) || 1} language={string(read.language) || path.split('.').pop()} /> : <div className={styles.notice}>{read.displayedLines === 0 && status === 'succeeded' ? t('emptyFile') : empty}</div>}{readNotice && <div className={styles.notice}>{readNotice}</div>}</>
          : Object.keys(search).length > 0 ? <><div className={styles.notice}>{t('searchCount', { matches: typeof search.matchCount === 'number' ? search.matchCount : '—', files: typeof search.fileCount === 'number' ? search.fileCount : files.length })}</div>{files.length ? <SearchActivity kind={string(search.kind)} basePath={string(search.basePath) || undefined} files={files.map(file => ({ path: string(file.path), matches: (Array.isArray(file.matches) ? file.matches.map(record) : []).map(match => ({ ...(typeof match.line === 'number' ? { line: match.line } : {}), text: string(match.text), context: match.context === true })) }))} /> : output ? <ActivityLines content={output} /> : <div className={styles.notice}>{empty}</div>}</>
          : changeVisible ? <><div className={styles.notice}>{t(status === 'running' ? 'proposed' : status === 'succeeded' ? 'applied' : 'unknown')}</div>{change.beforeAvailable === false && <div className={styles.notice}>{t('beforeUnavailable')}</div>}{['before_content_unavailable', 'before_permission_denied', 'before_content_too_large', 'before_content_not_text', 'patch_too_large', 'diff_too_large', 'diff_timeout', 'diff_unavailable'].includes(string(change.reason)) && <div className={styles.notice}>{t(`reasons.${string(change.reason)}`)}</div>}{patch && <ActivityLines content={patch} diff />}{counts && <div className={styles.notice}>+{counts.added} −{counts.removed} · {t('fileCount', { n: 1 })}</div>}{hasContent && <><h4 className={styles.section}>{t(status === 'running' ? 'proposedContent' : status === 'succeeded' ? 'writtenContent' : 'recordedContent')}</h4>{content ? <ActivityLines content={content} language={path.split('.').pop()} /> : <div className={styles.notice}>{t('emptyFile')}</div>}</>}{!patch && !hasContent && <div className={styles.notice}>{empty}</div>}</>
          : <><h4 className={styles.section}>{t('input')}</h4><ActivityLines content={input || t('unavailable')} /><h4 className={styles.section}>{t('output')}</h4><ActivityLines content={output || tool.error || empty} /></>}
        {tool.error && <div className={styles.failed}>{tool.error}</div>}
      </div>
    </div>
    {!research && <button type="button" className={styles.view} onClick={() => setView(true)}>{t('view')}</button>}
    {view && <Overlay open scope="window" onClose={() => setView(false)} className={styles.dialog} contentProps={{ role: 'dialog', 'aria-label': t('callDetails') }}><div className={styles.header}><strong className={styles.title}>{t('callDetails')} · {tool.name}</strong><button type="button" className={styles.action} onClick={() => setView(false)}>{t('close')}</button></div>{truncated && <div className={styles.notice}>{t('truncated')}</div>}<h4 className={styles.section}>{t('input')}</h4><pre className={styles.pre}>{input || t('unavailable')}</pre><h4 className={styles.section}>{t('output')}</h4><pre className={styles.pre}>{output || skillContent || tool.error || empty}</pre>{changeVisible && patch && <><h4 className={styles.section}>{t('changes')}</h4><ActivityLines content={patch} diff full /></>}{changeVisible && hasContent && <><h4 className={styles.section}>{t(status === 'succeeded' ? 'writtenContent' : 'recordedContent')}</h4><pre className={styles.pre}>{content}</pre></>}</Overlay>}
  </>;
}
