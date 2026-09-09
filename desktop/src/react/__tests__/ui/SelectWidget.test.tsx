// @vitest-environment jsdom

import React from 'react';
import { act, cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SelectWidget, type SelectOption } from '../../ui/SelectWidget';

const options: SelectOption[] = [
  { value: 'a', label: 'Option A' },
  { value: 'b', label: 'Option B' },
];

function openPopup(container: HTMLElement) {
  const trigger = container.querySelector('button') as HTMLButtonElement;
  act(() => {
    fireEvent.click(trigger);
  });
  return trigger;
}

function getPopup() {
  return document.body.querySelector('[data-select-widget-popup]');
}

describe('SelectWidget scroll-close scope', () => {
  afterEach(() => {
    cleanup();
  });

  it.each(['trigger', 'option'])('closes its own menu before an earlier document listener from %s', (focus) => {
    const outerEscape = vi.fn();
    document.addEventListener('keydown', outerEscape);
    try {
      const { container } = render(<SelectWidget options={options} value="a" onChange={() => {}} />);
      const trigger = openPopup(container);
      const target = focus === 'trigger' ? trigger : getPopup()!.querySelector('button')!;
      target.focus();
      fireEvent.keyDown(target, { key: 'Escape' });
      expect(getPopup()).toBeNull();
      expect(outerEscape).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(trigger);
    } finally {
      document.removeEventListener('keydown', outerEscape);
    }
  });

  it('closes only the dropdown on Escape and restores focus before the next Escape reaches the parent', () => {
    const onChange = vi.fn();
    const parentEscape = vi.fn();
    const handleParentKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') parentEscape();
    };
    window.addEventListener('keydown', handleParentKey);
    try {
      const { container } = render(
        <SelectWidget options={options} value="a" onChange={onChange} />,
      );
      const trigger = openPopup(container);
      const option = getPopup()?.querySelector('button') as HTMLButtonElement;
      option.focus();

      fireEvent.keyDown(option, { key: 'Escape' });

      expect(getPopup()).toBeNull();
      expect(document.activeElement).toBe(trigger);
      expect(trigger.getAttribute('aria-expanded')).toBe('false');
      expect(onChange).not.toHaveBeenCalled();
      expect(parentEscape).not.toHaveBeenCalled();

      fireEvent.keyDown(trigger, { key: 'Escape' });
      expect(parentEscape).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener('keydown', handleParentKey);
    }
  });

  it('stays open when an unrelated container (not an ancestor of the trigger) scrolls', () => {
    const { container } = render(
      <SelectWidget options={options} value="a" onChange={() => {}} />,
    );
    openPopup(container);
    expect(getPopup()).not.toBeNull();

    // A container elsewhere in the document — e.g. a background keep-alive chat panel — that
    // does not contain the trigger. Its scroll event must not close this popup.
    const unrelated = document.createElement('div');
    document.body.appendChild(unrelated);

    act(() => {
      unrelated.dispatchEvent(new Event('scroll', { bubbles: false }));
    });

    expect(getPopup()).not.toBeNull();
    unrelated.remove();
  });

  it('closes when an ancestor of the trigger scrolls', () => {
    const { container } = render(
      <SelectWidget options={options} value="a" onChange={() => {}} />,
    );
    const trigger = openPopup(container);
    expect(getPopup()).not.toBeNull();

    const ancestor = trigger.parentElement as HTMLElement;
    act(() => {
      ancestor.dispatchEvent(new Event('scroll', { bubbles: false }));
    });

    expect(getPopup()).toBeNull();
  });

  it('closes when the document scrolls', () => {
    const { container } = render(
      <SelectWidget options={options} value="a" onChange={() => {}} />,
    );
    openPopup(container);
    expect(getPopup()).not.toBeNull();

    act(() => {
      document.dispatchEvent(new Event('scroll', { bubbles: false }));
    });

    expect(getPopup()).toBeNull();
  });
});
