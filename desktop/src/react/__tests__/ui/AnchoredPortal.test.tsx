/** @vitest-environment jsdom */
import React, { useRef, useState } from 'react';
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { AnchoredPortal } from '../../ui/AnchoredPortal';

afterEach(cleanup);

function Menus() {
  const outerAnchor = useRef<HTMLButtonElement>(null);
  const innerAnchor = useRef<HTMLButtonElement>(null);
  const [outerOpen, setOuterOpen] = useState(false);
  const [innerOpen, setInnerOpen] = useState(false);
  return <>
    <button ref={outerAnchor} onClick={() => setOuterOpen(true)}>outer trigger</button>
    <AnchoredPortal open={outerOpen} anchorRef={outerAnchor} onClose={() => setOuterOpen(false)}>
      <input aria-label="outer input" />
      <button ref={innerAnchor} onClick={() => setInnerOpen(true)}>inner trigger</button>
      <AnchoredPortal open={innerOpen} anchorRef={innerAnchor} onClose={() => setInnerOpen(false)}>
        <input aria-label="inner input" onKeyDown={event => {
          if (event.shiftKey) event.preventDefault();
        }} />
      </AnchoredPortal>
    </AnchoredPortal>
  </>;
}

it.each(['trigger', 'input'])('Escape closes only the inner portal from its %s', (focus) => {
  const outerWindow = vi.fn();
  window.addEventListener('keydown', outerWindow);
  try {
    render(<Menus />);
    fireEvent.click(screen.getByText('outer trigger'));
    const anchor = screen.getByText('inner trigger');
    fireEvent.click(anchor);
    const target = focus === 'trigger' ? anchor : screen.getByLabelText('inner input');
    target.focus();
    fireEvent.keyDown(target, { key: 'Escape' });
    expect(screen.queryByLabelText('inner input')).not.toBeInTheDocument();
    expect(screen.getByLabelText('outer input')).toBeInTheDocument();
    expect(anchor).toHaveFocus();
    expect(outerWindow).not.toHaveBeenCalled();
  } finally {
    window.removeEventListener('keydown', outerWindow);
  }
});

it('respects Escape already handled by a nested input', () => {
  render(<Menus />);
  fireEvent.click(screen.getByText('outer trigger'));
  fireEvent.click(screen.getByText('inner trigger'));
  fireEvent.keyDown(screen.getByLabelText('inner input'), { key: 'Escape', shiftKey: true });
  expect(screen.getByLabelText('inner input')).toBeInTheDocument();
  expect(screen.getByLabelText('outer input')).toBeInTheDocument();
});
