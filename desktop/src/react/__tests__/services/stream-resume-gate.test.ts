/**
 * resume 增量重放信任门槛回归测试。
 *
 * canApplyIncrementalResume：只有当响应 streamId 与本地元数据一致、且断点
 * sinceSeq 是本地真实消费过的 seq 时，才允许把重放事件叠加到现有渲染状态；
 * 否则必须走 rebuild（先清空再 hydrate）。否则元数据失配后的一次普通切回/
 * 重连就会把整段 run 原样叠放到已渲染内容上。
 */

// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from 'vitest';
import {
  canApplyIncrementalResume,
  invalidateSessionStreamMeta,
  updateSessionStreamMeta,
} from '../../services/stream-resume';
import { useStore } from '../../stores';

const PATH = '/test/resume-gate.jsonl';
const SESSION_ID = 'sess_resume_gate';

function resetLocators(): void {
  useStore.setState({
    currentSessionId: null,
    currentSessionPath: null,
    sessions: [],
    sessionLocatorsById: {},
    streamingSessions: [],
  } as never);
  invalidateSessionStreamMeta();
  useStore.setState({
    sessionLocatorsById: { [SESSION_ID]: { path: PATH } },
    sessions: [{ path: PATH, sessionId: SESSION_ID, agentId: 'a1', agentName: 'Hana' }],
  } as never);
}

/** 让本地消费记录推进到 seq n（服务端协议：每条事件单调递增 seq）。 */
function markConsumedThrough(n: number): void {
  for (let seq = 1; seq <= n; seq += 1) {
    updateSessionStreamMeta({ sessionId: SESSION_ID, sessionPath: PATH, streamId: 's1', seq });
  }
}

describe('canApplyIncrementalResume', () => {
  beforeEach(resetLocators);

  it('同一条流且断点已被本地消费 → 允许增量', () => {
    markConsumedThrough(5);
    expect(canApplyIncrementalResume(
      { streamId: 's1', sinceSeq: 5, events: [{ seq: 6 }] },
      PATH,
    )).toBe(true);
  });

  it('流 ID 不一致（meta 失效/劈叉）→ 拒绝增量，要求重建', () => {
    markConsumedThrough(5);
    invalidateSessionStreamMeta();
    expect(canApplyIncrementalResume(
      { streamId: 's1', sinceSeq: 5, events: [{ seq: 6 }] },
      PATH,
    )).toBe(false);
  });

  it('断点 seq 从未消费（存在缺口）→ 拒绝增量', () => {
    // 本地只连续消费到 3，然后跳到 7；断点 6 是本地从未见过的位置
    markConsumedThrough(3);
    updateSessionStreamMeta({ sessionId: SESSION_ID, sessionPath: PATH, streamId: 's1', seq: 7 });
    updateSessionStreamMeta({ sessionId: SESSION_ID, sessionPath: PATH, streamId: 's1', seq: 8 });
    expect(canApplyIncrementalResume(
      { streamId: 's1', sinceSeq: 6, events: [{ seq: 7 }] },
      PATH,
    )).toBe(false);
  });

  it('sinceSeq=0 全量补发只对空白本地状态安全', () => {
    updateSessionStreamMeta({ sessionId: SESSION_ID, sessionPath: PATH, streamId: 's1' });
    expect(canApplyIncrementalResume(
      { streamId: 's1', sinceSeq: 0, events: [{ seq: 1 }] },
      PATH,
    )).toBe(true);

    markConsumedThrough(2);
    expect(canApplyIncrementalResume(
      { streamId: 's1', sinceSeq: 0, events: [{ seq: 1 }, { seq: 2 }, { seq: 3 }] },
      PATH,
    )).toBe(false);
  });

  it('响应缺 streamId 或缺 sinceSeq → 一律拒绝增量', () => {
    markConsumedThrough(4);
    expect(canApplyIncrementalResume({ sinceSeq: 4, events: [{ seq: 5 }] }, PATH)).toBe(false);
    expect(canApplyIncrementalResume({ streamId: 's1', events: [{ seq: 5 }] }, PATH)).toBe(false);
  });
});
