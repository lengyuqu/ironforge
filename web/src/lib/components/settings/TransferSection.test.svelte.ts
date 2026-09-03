import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const reposTransfer = vi.fn();
const goto = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  repos: { transfer: (...a: unknown[]) => reposTransfer(...a) },
}));

vi.mock('$app/navigation', () => ({
  goto: (...a: unknown[]) => goto(...a),
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import TransferSection from './TransferSection.svelte';

const confirmMock = vi.fn(() => true);

function renderSection() {
  render(TransferSection, { owner: 'alice', repo: 'demo' });
}

describe('TransferSection.svelte', () => {
  beforeEach(() => {
    reposTransfer.mockReset().mockResolvedValue(undefined);
    goto.mockClear();
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('keeps the transfer button disabled until a new owner is entered', () => {
    renderSection();

    const button = screen.getByText('settings.transfer.confirm');
    expect(button).toBeDisabled();

    fireEvent.input(screen.getByLabelText('settings.transfer.new_owner'), {
      target: { value: 'bob' },
    });
    expect(button).toBeEnabled();
  });

  it('transfers to the trimmed owner after confirmation and shows the success box', async () => {
    renderSection();

    fireEvent.input(screen.getByLabelText('settings.transfer.new_owner'), {
      target: { value: '  bob  ' },
    });
    await fireEvent.click(screen.getByText('settings.transfer.confirm'));

    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() =>
      expect(reposTransfer).toHaveBeenCalledWith('alice', 'demo', 'bob')
    );
    // Success is shown inline; the redirect to the new repo path is
    // deferred by a 1.5s setTimeout, which is not awaited here.
    await waitFor(() =>
      expect(document.querySelector('.success-box')?.textContent).toBe('settings.transfer.success')
    );
    expect(document.querySelector('.error-box')).not.toBeInTheDocument();
  });

  it('skips the transfer when the confirmation dialog is dismissed', async () => {
    confirmMock.mockReturnValue(false);
    renderSection();

    fireEvent.input(screen.getByLabelText('settings.transfer.new_owner'), {
      target: { value: 'bob' },
    });
    await fireEvent.click(screen.getByText('settings.transfer.confirm'));

    expect(confirmMock).toHaveBeenCalled();
    expect(reposTransfer).not.toHaveBeenCalled();
  });

  it('surfaces a transfer failure in the inline error box', async () => {
    reposTransfer.mockRejectedValueOnce(new Error('target is an organization'));
    renderSection();

    fireEvent.input(screen.getByLabelText('settings.transfer.new_owner'), {
      target: { value: 'bob' },
    });
    await fireEvent.click(screen.getByText('settings.transfer.confirm'));

    await waitFor(() =>
      expect(screen.getByText('target is an organization')).toBeInTheDocument()
    );
    expect(document.querySelector('.success-box')).not.toBeInTheDocument();
  });
});
