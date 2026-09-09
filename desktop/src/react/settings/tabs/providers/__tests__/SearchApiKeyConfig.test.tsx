/** @vitest-environment jsdom */
import React from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../../store';

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), reload: vi.fn() }));
vi.mock('../../../api', () => ({ lingxiFetch: (...args: unknown[]) => mocks.fetch(...args) }));
vi.mock('../../../actions', () => ({ loadSettingsConfig: () => mocks.reload() }));
vi.mock('../../../helpers', () => ({ t: (key: string) => key }));
vi.mock('../../../widgets/KeyInput', () => ({ KeyInput: () => <input /> }));
import { SearchApiKeyConfig } from '../SearchApiKeyConfig';

beforeEach(() => {
  vi.clearAllMocks();
  useSettingsStore.setState({ globalModelsConfig: { search: { provider: 'tavily', api_keys: { tavily: 'test-key' } } } as any });
});
afterEach(cleanup);

it.each(['success', 'failure', 'rejection'])('shows verification progress and recovers after %s', async (result) => {
  let complete!: (value: unknown) => void;
  let reject!: (error: Error) => void;
  mocks.fetch.mockReturnValue(new Promise((resolve, rejectPromise) => { complete = resolve; reject = rejectPromise; }));
  render(<SearchApiKeyConfig />);
  const button = screen.getByRole('button', { name: 'settings.search.verify' });
  fireEvent.click(button);
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute('aria-busy', 'true');
  expect(button).toHaveTextContent('settings.search.verifying');
  fireEvent.click(button);
  expect(mocks.fetch).toHaveBeenCalledTimes(1);
  if (result === 'rejection') reject(new Error('offline'));
  else complete({ json: async () => ({ ok: result === 'success' }) });
  await waitFor(() => expect(button).toBeEnabled());
  expect(button).toHaveTextContent('settings.search.verify');
  expect(button).not.toHaveAttribute('aria-busy', 'true');
});
