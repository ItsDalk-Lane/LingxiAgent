// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { KnowledgeCitationPreview } from '../../components/chat/KnowledgeCitationPreview';
import { useKnowledgeCitationResource } from '../../components/chat/knowledge-citation-scope';

vi.mock('../../components/chat/knowledge-citation-scope', () => ({ useKnowledgeCitationResource: vi.fn() }));
afterEach(() => { cleanup(); document.querySelectorAll('[data-test-citation-anchor]').forEach(node => node.remove()); vi.resetAllMocks(); });

function setup(state: ReturnType<typeof useKnowledgeCitationResource>) {
  vi.mocked(useKnowledgeCitationResource).mockReturnValue(state);
  const anchor = document.createElement('a');
  anchor.setAttribute('data-test-citation-anchor', ''); anchor.textContent = '1'; document.body.append(anchor);
  const onEnter = vi.fn(), onLeave = vi.fn(), onClose = vi.fn();
  const view = render(<KnowledgeCitationPreview citationId="cite_original" anchor={anchor} id="preview" onEnter={onEnter} onLeave={onLeave} onClose={onClose} />);
  return { anchor, onEnter, onLeave, onClose, view };
}

it('悬浮内容只显示保存的原文及文件名，鼠标可移入，关闭后清除无障碍关联', () => {
  const f = setup({ resolved: { source: { displayName: '/private/资料/制度.txt' }, citation: { canonicalText: '原文规定：每年十五天。' } } as ReturnType<typeof useKnowledgeCitationResource>['resolved'], failed: false, retry: vi.fn() });
  const preview = screen.getByRole('tooltip');
  expect(preview).toHaveTextContent('制度.txt');
  expect(preview).toHaveTextContent('原文规定：每年十五天。');
  expect(preview).not.toHaveTextContent('/private/资料');
  expect(f.anchor).toHaveAttribute('aria-describedby', 'preview');
  fireEvent.mouseEnter(preview); expect(f.onEnter).toHaveBeenCalledOnce();
  fireEvent.mouseLeave(preview); expect(f.onLeave).toHaveBeenCalledOnce();
  fireEvent.keyDown(preview, { key: 'Escape' }); expect(f.onClose).toHaveBeenCalledOnce();
  f.view.unmount(); expect(f.anchor).not.toHaveAttribute('aria-describedby');
});

it('原文读取失败明确展示错误并允许重试，不伪造引用内容', () => {
  const retry = vi.fn(); setup({ resolved: null, failed: true, retry });
  expect(screen.getByRole('alert')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button')); expect(retry).toHaveBeenCalledOnce();
  expect(screen.getByRole('tooltip')).not.toHaveTextContent('原文规定');
});
