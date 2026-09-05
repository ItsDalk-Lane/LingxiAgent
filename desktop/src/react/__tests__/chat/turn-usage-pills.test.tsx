/**
 * 用量/用时胶囊 · 组件渲染测试（任务2 验收 a/b/c 组 + 弹窗交互）。
 *
 * jsdom + testing-library：有 usage 数据渲染双胶囊；无数据整体不渲染且不出现 0；
 * 弹窗行按数据有无条件渲染（缓存写入 >0 才显示、缓存命中/其中推理无数据不显示、
 * TTFT 行永不出现）；千分位精确值；点开/Esc/外点关闭交互。
 */
// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { TurnUsagePills } from '../../components/chat/TurnUsagePills';
import { MessageFooterActions } from '../../components/chat/MessageFooterActions';
import type { TurnUsageStats } from '../../components/chat/turn-usage';

function statsFixture(overrides: Partial<TurnUsageStats> = {}): TurnUsageStats {
  return {
    uncachedInputTokens: 1234567,
    cacheReadTokens: 500,
    cacheWriteTokens: 25,
    outputTokens: 40,
    reasoningTokens: 10,
    totalTokens: 12200,
    cacheHitPercent: '83.3',
    modelLabels: ['openai/gpt-test'],
    runMs: 65000,
    tokensPerSecond: 25,
    ttftMs: 800,
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
});

describe('用量/用时胶囊渲染（验收 a：有数据渲染）', () => {
  it('渲染双胶囊：紧凑总量标签 + 整秒用时标签', () => {
    const { container } = render(<TurnUsagePills stats={statsFixture()} />);
    expect(screen.getByTestId('turn-usage-pill')).toHaveTextContent('用量 12.2K tok');
    expect(screen.getByTestId('turn-time-pill')).toHaveTextContent('用时 1分05秒');
    // 初始不渲染弹窗
    expect(screen.queryByTestId('turn-usage-dialog')).toBeNull();
    expect(screen.queryByTestId('turn-time-dialog')).toBeNull();
    expect(container.querySelector('[data-testid="turn-usage-pill"]')).not.toBeNull();
  });

  it('点击用量胶囊弹出明细弹窗（portal 到 body），Esc 关闭', () => {
    render(<TurnUsagePills stats={statsFixture()} />);
    fireEvent.click(screen.getByTestId('turn-usage-pill'));
    const dialog = screen.getByTestId('turn-usage-dialog');
    expect(dialog).toHaveAttribute('role', 'dialog');
    expect(screen.getByText('本轮用量')).toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('turn-usage-dialog')).toBeNull();
  });

  it('外点关闭、再次点击触发钮切换开合', () => {
    render(<TurnUsagePills stats={statsFixture()} />);
    fireEvent.click(screen.getByTestId('turn-time-pill'));
    expect(screen.getByTestId('turn-time-dialog')).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByTestId('turn-time-dialog')).toBeNull();
    fireEvent.click(screen.getByTestId('turn-time-pill'));
    expect(screen.getByTestId('turn-time-dialog')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('turn-time-pill'));
    expect(screen.queryByTestId('turn-time-dialog')).toBeNull();
  });
});

describe('无数据不渲染（验收 b）', () => {
  it('totalTokens 无事实 → 整体返回 null，不渲染任何胶囊与 0', () => {
    const { container } = render(
      <TurnUsagePills stats={statsFixture({ totalTokens: null })} />,
    );
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId('turn-usage-pill')).toBeNull();
    expect(screen.queryByTestId('turn-time-pill')).toBeNull();
    expect(container.textContent).not.toContain('0');
  });

  it('runMs 无事实（历史轮账本缺 ended_at）→ 用时胶囊不渲染，用量胶囊保留', () => {
    render(<TurnUsagePills stats={statsFixture({ runMs: null })} />);
    expect(screen.getByTestId('turn-usage-pill')).toBeInTheDocument();
    expect(screen.queryByTestId('turn-time-pill')).toBeNull();
  });
});

describe('用量弹窗行条件渲染（验收 c）', () => {
  it('全数据：模型/缓存命中/未缓存输入/缓存读取/缓存写入/输出+其中推理 全在，精确值千分位', () => {
    render(<TurnUsagePills stats={statsFixture()} />);
    fireEvent.click(screen.getByTestId('turn-usage-pill'));
    expect(screen.getByText('提供方 / 模型')).toBeInTheDocument();
    expect(screen.getByText('openai/gpt-test')).toBeInTheDocument();
    expect(screen.getByText('缓存命中')).toBeInTheDocument();
    expect(screen.getByText('83.3%')).toBeInTheDocument();
    expect(screen.getByText('未缓存输入')).toBeInTheDocument();
    expect(screen.getByText('1,234,567 tok')).toBeInTheDocument();
    expect(screen.getByText('缓存读取')).toBeInTheDocument();
    expect(screen.getByText('500 tok')).toBeInTheDocument();
    expect(screen.getByText('缓存写入')).toBeInTheDocument();
    expect(screen.getByText('25 tok')).toBeInTheDocument();
    expect(screen.getByText('输出')).toBeInTheDocument();
    expect(screen.getByText('（其中推理 10 tok）')).toBeInTheDocument();
    // 标题值 = 总量千分位
    expect(screen.getByText('12,200 tok')).toBeInTheDocument();
  });

  it('缓存写入 0 / 缓存读取无事实 / 推理无事实 → 对应行整行不显示，不冒充 0', () => {
    render(<TurnUsagePills stats={statsFixture({
      cacheWriteTokens: 0,
      cacheReadTokens: null,
      cacheHitPercent: null,
      reasoningTokens: null,
    })} />);
    fireEvent.click(screen.getByTestId('turn-usage-pill'));
    expect(screen.queryByText('缓存写入')).toBeNull();
    expect(screen.queryByText('缓存读取')).toBeNull();
    expect(screen.queryByText('缓存命中')).toBeNull();
    expect(screen.queryByText(/其中推理/)).toBeNull();
    // 仍有事实的行照常
    expect(screen.getByText('未缓存输入')).toBeInTheDocument();
    expect(screen.getByText('输出')).toBeInTheDocument();
  });
});

describe('用时弹窗（验收 c 补充：TTFT 永不显示，TPS 按有无渲染）', () => {
  it('有 TPS：显示本轮总用时 + 输出速度（TPS）+ 首 token 用时（TTFT）', () => {
    render(<TurnUsagePills stats={statsFixture()} />);
    fireEvent.click(screen.getByTestId('turn-time-pill'));
    expect(screen.getByText('本轮用时和速度')).toBeInTheDocument();
    expect(screen.getByText('本轮总用时')).toBeInTheDocument();
    expect(screen.getByText('输出速度（TPS）')).toBeInTheDocument();
    expect(screen.getByText('25 tok/s')).toBeInTheDocument();
    expect(screen.getByText('首 token 用时（TTFT）')).toBeInTheDocument();
    expect(screen.getByText('0.8秒')).toBeInTheDocument();
    expect(screen.getByText('1分05秒')).toBeInTheDocument();
  });

  it('无 TPS（缺时长事实）：仅本轮总用时行', () => {
    render(<TurnUsagePills stats={statsFixture({ tokensPerSecond: null })} />);
    fireEvent.click(screen.getByTestId('turn-time-pill'));
    expect(screen.getByText('本轮总用时')).toBeInTheDocument();
    expect(screen.queryByText('输出速度（TPS）')).toBeNull();
    expect(screen.queryByText(/tok\/s/)).toBeNull();
  });

  it('无 TTFT 事实（ttftMs=null）→ 首 token 行整行不显示', () => {
    render(<TurnUsagePills stats={statsFixture({ ttftMs: null })} />);
    fireEvent.click(screen.getByTestId('turn-time-pill'));
    expect(screen.getByText('本轮总用时')).toBeInTheDocument();
    expect(screen.queryByText('首 token 用时（TTFT）')).toBeNull();
  });
});

describe('页脚胶囊与时间的行为统一（平时隐藏，悬停显示）', () => {
  it('timePersistent 场景下 statsNode/time 获得隐藏包装类，随按钮一同被 CSS 隐藏', () => {
    const { container } = render(
      <MessageFooterActions
        timeText="19:36"
        timePersistent
        statsNode={<span data-testid="stats-wrapper-probe">pills</span>}
        actions={[]}
      />,
    );
    // persistent 行挂隐藏容器类（CSS 按 .messageFooterActionsTimePersistent 前缀隐藏/悬停显示）
    const row = container.querySelector('[data-message-actions]');
    expect(row?.className).toContain('messageFooterActionsTimePersistent');
    const stats = container.querySelector('[class*="messageFooterStats"]');
    expect(stats).not.toBeNull();
    expect(stats?.textContent).toContain('pills');
    const time = container.querySelector('[class*="messageFooterTime"]');
    expect(time).not.toBeNull();
  });
});
