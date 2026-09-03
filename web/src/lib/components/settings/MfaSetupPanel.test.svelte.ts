import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const mfaEnable = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  mfa: { enable: (...a: unknown[]) => mfaEnable(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

import MfaSetupPanel from './MfaSetupPanel.svelte';
import type { MfaSetupResponse } from '$lib/api/mfa';

function renderPanel() {
  const onEnabled = vi.fn().mockResolvedValue(undefined);
  const setup: MfaSetupResponse = {
    secret: 'JBSWY3DPEHPK3PXP',
    otpauth_url: 'otpauth://totp/demo',
    qr_svg: '<svg data-qr><rect/></svg>',
  };
  render(MfaSetupPanel, { setup, onEnabled });
  return { onEnabled };
}

describe('MfaSetupPanel.svelte', () => {
  beforeEach(() => {
    mfaEnable.mockReset().mockResolvedValue({ backup_codes: ['A-1', 'B-2'] });
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('renders the QR svg, the secret and a disabled submit button', () => {
    renderPanel();

    expect(document.querySelector('[data-qr]')).toBeInTheDocument();
    expect(screen.getByText('JBSWY3DPEHPK3PXP')).toBeInTheDocument();
    expect(screen.getByText('Enable MFA')).toBeDisabled();
  });

  it('enables MFA with the trimmed code and passes backup codes upward', async () => {
    const { onEnabled } = renderPanel();

    fireEvent.input(screen.getByLabelText('Authentication code'), {
      target: { value: '  123456  ' },
    });
    expect(screen.getByText('Enable MFA')).toBeEnabled();

    await fireEvent.submit(screen.getByText('Enable MFA').closest('form')!);

    await waitFor(() => expect(mfaEnable).toHaveBeenCalledWith('123456'));
    await waitFor(() => expect(onEnabled).toHaveBeenCalledWith(['A-1', 'B-2']));
  });

  it('blocks empty codes with an error toast and no API call', async () => {
    const { onEnabled } = renderPanel();

    await fireEvent.submit(screen.getByText('Enable MFA').closest('form')!);

    expect(toastError).toHaveBeenCalledWith('Authentication code is required');
    expect(mfaEnable).not.toHaveBeenCalled();
    expect(onEnabled).not.toHaveBeenCalled();
  });

  it('surfaces enable failures via an error toast', async () => {
    mfaEnable.mockRejectedValueOnce(new Error('code mismatch'));
    renderPanel();

    fireEvent.input(screen.getByLabelText('Authentication code'), {
      target: { value: '000000' },
    });
    await fireEvent.submit(screen.getByText('Enable MFA').closest('form')!);

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('code mismatch'));
  });
});
