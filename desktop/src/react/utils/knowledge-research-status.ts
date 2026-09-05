/** 只把已知停止原因转换成界面文案，不展示内部错误标记。 */
export function knowledgeResearchStopNote(reason: unknown, translate: (key: string) => string): string | undefined {
  if (typeof reason !== 'string' || !reason || reason === 'complete' || reason === 'completed') return undefined;
  const keys: Record<string, string> = {
    tool_budget_exhausted: 'knowledgeResearchStopTools',
    wall_clock_exhausted: 'knowledgeResearchStopTime',
    round_budget_exhausted: 'knowledgeResearchStopRounds',
    max_rounds: 'knowledgeResearchStopRounds',
    no_evidence_progress: 'knowledgeResearchStopNoProgress',
    no_progress: 'knowledgeResearchStopNoProgress',
    cancelled: 'knowledgeResearchCancelled',
    KNOWLEDGE_RESEARCH_CANCELLED: 'knowledgeResearchCancelled',
    critical_tools_unavailable: 'knowledgeResearchStopUnavailable',
    agent_protocol_failure: 'knowledgeResearchStopIncomplete',
    KNOWLEDGE_RESEARCH_PROTOCOL_FAILED: 'knowledgeResearchStopIncomplete',
  };
  return translate(`chat.${keys[reason] ?? 'knowledgeResearchInterrupted'}`);
}
