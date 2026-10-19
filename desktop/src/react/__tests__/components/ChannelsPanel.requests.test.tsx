// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChannelsPanel, ChannelInput, ChannelMessages, ChannelAgentSettingsPanel, AgentPhoneSessionPreview } from '../../components/ChannelsPanel';
import { ChannelList } from '../../components/channels/ChannelList';
import { useStore } from '../../stores';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { fetchConfig } from '../../hooks/use-config';
import { loadMessages } from '../../stores/session-actions';

vi.mock('../../hooks/use-hana-fetch', () => ({ lingxiFetch: vi.fn() }));
vi.mock('../../hooks/use-config', () => ({ fetchConfig: vi.fn() }));
vi.mock('../../stores/session-actions', async original => ({ ...await original<typeof import('../../stores/session-actions')>(), loadMessages: vi.fn() }));

const channel = { id: 'ch1', name: 'crew', members: ['a', 'b'], lastMessage: '', lastSender: '',
  lastTimestamp: '', newMessageCount: 0, isDM: false };

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(loadMessages).mockReset();
  window.t = ((key: string) => key) as typeof window.t;
  useStore.setState({ serverPort: '3210', activeServerConnection: null, activeServerConnectionId: null,
    currentAgentId: 'a', currentChannel: 'ch1', agents: [], channelIsDM: false,
    channelMembers: ['a', 'b'], channels: [channel], channelMessages: [], channelMessageCache: {},
    channelHistoryError: null, channelHistoryLoading: false, channelListError: null, channelListLoading: false,
    channelSettingsError: null, channelSettingsLoading: false, models: [{ id: 'test', provider: 'test', name: 'Test' }],
    toasts: [] } as never);
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('频道读取错误与重试界面', () => {
  it('历史失败显示重试，不能显示暂无消息', () => {
    useStore.setState({ channelHistoryError: 'offline' });
    render(<ChannelMessages />);
    expect(screen.getByRole('alert')).toHaveTextContent('channel.loadFailed');
    expect(screen.getByRole('button', { name: 'channel.retry' })).toBeInTheDocument();
    expect(screen.queryByText('channel.noMessages')).not.toBeInTheDocument();
  });

  it('历史失败保留已读缓存，并可从原会话重试', async () => {
    useStore.setState({ channelHistoryError: 'offline', channelMessages: [{ sender: 'user', timestamp: 'now', body: 'cached content' }] });
    vi.mocked(lingxiFetch).mockRejectedValue(new Error('still offline'));
    render(<ChannelMessages />);
    expect(screen.getByText('cached content')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'channel.retry' }));
    await waitFor(() => { expect(lingxiFetch).toHaveBeenCalledWith('/api/channels/ch1'); });
  });

  it('目录失败不显示无频道，保留目录时仍显示失败提示', () => {
    useStore.setState({ channels: [], channelListError: 'offline' });
    const { rerender } = render(<ChannelList />);
    expect(screen.getByRole('alert')).toHaveTextContent('channel.loadFailed');
    expect(screen.queryByText('channel.empty')).not.toBeInTheDocument();
    act(() => { useStore.setState({ channels: [channel] }); });
    rerender(<ChannelList />);
    expect(screen.getByText('crew')).toBeInTheDocument();
    expect(screen.getByRole('alert')).toBeInTheDocument();
  });

  it('未读到设置时不展示来自旧频道的工具权限选项', () => {
    useStore.setState({ channelSettingsError: 'offline', channelAgentPhoneToolMode: 'write' });
    render(<ChannelAgentSettingsPanel />);
    expect(screen.getByRole('alert')).toHaveTextContent('channel.loadFailed');
    expect(screen.queryByText('channel.toolWrite')).not.toBeInTheDocument();
  });

  it('发送失败保留输入内容并提示，不能显示假消息', async () => {
    vi.mocked(lingxiFetch).mockRejectedValueOnce(new Error('offline'));
    render(<ChannelInput />);
    const input = screen.getByPlaceholderText('channel.inputPlaceholder');
    fireEvent.change(input, { target: { value: 'keep my message' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => { expect(useStore.getState().toasts.at(-1)?.text).toBe('channel.sendFailed'); });
    expect(input).toHaveValue('keep my message');
    expect(useStore.getState().channelMessages).toEqual([]);
  });

  it('切换助手后旧配置读取不能开启新助手的频道', async () => {
    let resolveOld!: (value: unknown) => void;
    vi.mocked(fetchConfig).mockReturnValueOnce(new Promise(done => { resolveOld = done; }))
      .mockResolvedValueOnce({ channels: { enabled: false } });
    render(<ChannelsPanel />);
    act(() => { useStore.setState({ currentAgentId: 'b' }); });
    await waitFor(() => { expect(useStore.getState().channelsEnabled).toBe(false); });
    await act(async () => { resolveOld({ channels: { enabled: true } }); });
    expect(useStore.getState().channelsEnabled).toBe(false);
    expect(lingxiFetch).not.toHaveBeenCalled();
  });
  it('初始配置读取失败后，重试先重新确认开关再加载目录', async () => {
    vi.mocked(fetchConfig).mockRejectedValueOnce(new Error('config offline'));
    vi.mocked(lingxiFetch).mockImplementation(async path => new Response(JSON.stringify(
      path === '/api/config' ? { channels: { enabled: true } }
        : path === '/api/channels' ? { channels: [] } : { dms: [] },
    )));
    render(<><ChannelsPanel /><ChannelList /></>);
    await screen.findByRole('alert');
    fireEvent.click(screen.getByRole('button', { name: 'channel.retry' }));
    await waitFor(() => { expect(useStore.getState().channelsEnabled).toBe(true); });
    await screen.findByText('channel.empty');
    expect(vi.mocked(lingxiFetch).mock.calls.map(([path]) => path)).toEqual(['/api/config', '/api/channels', '/api/dm']);
  });

  it('电话会话历史读取失败明确提示，重试完成后结束加载状态', async () => {
    vi.mocked(loadMessages).mockRejectedValueOnce(new Error('history offline')).mockResolvedValueOnce(undefined);
    render(<AgentPhoneSessionPreview sessionPath="native:phone-preview" agentId="a" />);
    await screen.findByRole('alert');
    expect(screen.queryByText('channel.agentIdle')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'channel.retry' }));
    await screen.findByText('channel.agentIdle');
    expect(loadMessages).toHaveBeenCalledTimes(2);
    expect(screen.queryByText('common.loading')).not.toBeInTheDocument();
  });

});
