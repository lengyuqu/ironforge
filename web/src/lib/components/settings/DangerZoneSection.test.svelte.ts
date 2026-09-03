import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const reposDelete = vi.fn();
const goto = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  repos: { delete: (...a: unknown[]) => reposDelete(...a) },
}));

vi.mock('$app/navigation', () => ({
  goto: (...a: unknown[]) => goto(...a),
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import DangerZoneSection from './DangerZoneSection.svelte';

const confirmMock = vi.fn(() => true);

function renderSection() {
  render(DangerZoneSection, { owner: 'alice', repo: 'demo' });
}

describe('DangerZoneSection.svelte', () => {
  beforeEach(() => {
    reposDelete.mockReset().mockResolvedValue(undefined);
    goto.mockClear();
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('keeps the delete button disabled until the exact repo path is typed', () => {
    renderSection();

    const input = screen.getByLabelText(/settings.delete.confirm_instruction/);
    const button = screen.getByText('settings.delete.confirm_button');

    // Empty and wrong confirmations both disable the button.
    expect(button).toBeDisabled();
    fireEvent.input(input, { target: { value: 'demo' } });
    expect(button).toBeDisabled();
    fireEvent.input(input, { target: { value: 'alice/demo' } });
    expect(button).toBeEnabled();
  });

  it('deletes the repo and redirects to the dashboard after confirmation', async () => {
    renderSection();

    fireEvent.input(screen.getByLabelText(/settings.delete.confirm_instruction/), {
      target: { value: 'alice/demo' },
    });
    await fireEvent.click(screen.getByText('settings.delete.confirm_button'));

    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() => expect(reposDelete).toHaveBeenCalledWith('alice', 'demo'));
    await waitFor(() => expect(goto).toHaveBeenCalledWith('/dashboard'));
  });

  it('skips deletion when the confirmation dialog is dismissed', async () => {
    confirmMock.mockReturnValue(false);
    renderSection();

    fireEvent.input(screen.getByLabelText(/settings.delete.confirm_instruction/), {
      target: { value: 'alice/demo' },
    });
    await fireEvent.click(screen.getByText('settings.delete.confirm_button'));

    expect(confirmMock).toHaveBeenCalled();
    expect(reposDelete).not.toHaveBeenCalled();
    expect(goto).not.toHaveBeenCalled();
  });

  it('surfaces the delete failure inline without redirecting', async () => {
    reposDelete.mockRejectedValueOnce(new Error('still has forks'));
    renderSection();

    fireEvent.input(screen.getByLabelText(/settings.delete.confirm_instruction/), {
      target: { value: 'alice/demo' },
    });
    await fireEvent.click(screen.getByText('settings.delete.confirm_button'));

    await waitFor(() => expect(screen.getByText('still has forks')).toBeInTheDocument());
    expect(goto).not.toHaveBeenCalled();
  });
});
