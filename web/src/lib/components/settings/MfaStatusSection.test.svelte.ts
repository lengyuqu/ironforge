import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const mfaDisable = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  mfa: { disable: (...a: unknown[]) => mfaDisable(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

import MfaStatusSection from './MfaStatusSection.svelte';
import type { MfaBackupStatus } from '$lib/api/mfa';

const confirmMock = vi.fn(() => true);

function makeStatus(total = 8, unused = 5): MfaBackupStatus {
  return {
    total,
    unused,
    codes: Array.from({ length: total }, () => ({
      used: true,
      used_at: null,
      created_at: '2026-09-01T00:00:00Z',
    })),
  };
}

function renderSection(backupStatus: MfaBackupStatus | null = makeStatus()) {
  const onStartSetup = vi.fn().mockResolvedValue(undefined);
  const onDisabled = vi.fn().mockResolvedValue(undefined);
  render(MfaStatusSection, { backupStatus, onStartSetup, onDisabled });
  return { onStartSetup, onDisabled };
}

describe('MfaStatusSection.svelte', () => {
  beforeEach(() => {
    mfaDisable.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('shows the disabled state with a setup button when MFA is off', () => {
    const { onStartSetup } = renderSection(null);

    expect(screen.getByText('Disabled')).toBeInTheDocument();
    expect(screen.getByText('Set up MFA')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Set up MFA'));
    expect(onStartSetup).toHaveBeenCalledTimes(1);
  });

  it('shows the enabled state with backup code counters', () => {
    renderSection(makeStatus(8, 5));

    const status = document.querySelector('.status');
    expect(status?.textContent).toBe('Enabled');
    expect(status?.classList.contains('enabled')).toBe(true);
    expect(screen.getByText('5')).toBeInTheDocument();
    expect(screen.getByText('8')).toBeInTheDocument();
    expect(screen.queryByText('Set up MFA')).not.toBeInTheDocument();
  });

  it('disables MFA with the current password after confirmation', async () => {
    const { onDisabled } = renderSection();

    fireEvent.input(screen.getByLabelText('Current password'), {
      target: { value: 'hunter2' },
    });
    await fireEvent.submit(screen.getByText('Disable MFA').closest('form')!);

    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() => expect(mfaDisable).toHaveBeenCalledWith('hunter2'));
    expect(toastSuccess).toHaveBeenCalledWith('MFA disabled');
    await waitFor(() => expect(onDisabled).toHaveBeenCalledTimes(1));
  });

  it('blocks disabling without a password or on confirm dismissal', async () => {
    const { onDisabled } = renderSection();

    await fireEvent.submit(screen.getByText('Disable MFA').closest('form')!);
    expect(toastError).toHaveBeenCalledWith('Current password is required');
    expect(mfaDisable).not.toHaveBeenCalled();

    confirmMock.mockReturnValue(false);
    fireEvent.input(screen.getByLabelText('Current password'), {
      target: { value: 'hunter2' },
    });
    await fireEvent.submit(screen.getByText('Disable MFA').closest('form')!);
    expect(mfaDisable).not.toHaveBeenCalled();
    expect(onDisabled).not.toHaveBeenCalled();
  });

  it('surfaces disable failures via an error toast', async () => {
    mfaDisable.mockRejectedValueOnce(new Error('wrong password'));
    renderSection();

    fireEvent.input(screen.getByLabelText('Current password'), {
      target: { value: 'wrong' },
    });
    await fireEvent.submit(screen.getByText('Disable MFA').closest('form')!);

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('wrong password'));
  });
});
