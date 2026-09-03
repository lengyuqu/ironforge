import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const runnersRegister = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  runners: { register: (...a: unknown[]) => runnersRegister(...a) },
}));

import RunnerRegisterForm from './RunnerRegisterForm.svelte';

describe('RunnerRegisterForm.svelte', () => {
  beforeEach(() => {
    runnersRegister.mockReset();
  });

  it('disables the register button until a name is entered', () => {
    render(RunnerRegisterForm, { onRegistered: () => {} });

    const registerBtn = screen.getAllByText('admin.runners.register')[1] as HTMLButtonElement;
    expect(registerBtn.disabled).toBe(true);

    fireEvent.input(screen.getByPlaceholderText('linux-runner-01'), {
      target: { value: 'win-runner-02' },
    });
    expect(registerBtn.disabled).toBe(false);
  });

  it('registers with trimmed name and split labels, then shows the token banner', async () => {
    runnersRegister.mockResolvedValue({ id: 9, token: 'secret-token' });
    const onRegistered = vi.fn().mockResolvedValue(undefined);
    render(RunnerRegisterForm, { onRegistered });

    await fireEvent.input(screen.getByPlaceholderText('linux-runner-01'), {
      target: { value: '  win-runner-02 ' },
    });
    await fireEvent.input(screen.getByPlaceholderText('linux,x86_64,docker'), {
      target: { value: ' windows, x86_64 ,,' },
    });
    await fireEvent.click(screen.getAllByText('admin.runners.register')[1]);

    await waitFor(() =>
      expect(runnersRegister).toHaveBeenCalledWith({
        name: 'win-runner-02',
        labels: ['windows', 'x86_64'],
      })
    );
    await waitFor(() => expect(onRegistered).toHaveBeenCalledTimes(1));
    // One-time token banner with copy button.
    expect(await screen.findByText('secret-token')).toBeInTheDocument();
    expect(screen.getByText('common.copy')).toBeInTheDocument();
    // The form is cleared after success.
    expect((screen.getByPlaceholderText('linux-runner-01') as HTMLInputElement).value).toBe('');
  });

  it('shows an inline error when registration fails', async () => {
    runnersRegister.mockRejectedValue(new Error('name taken'));
    render(RunnerRegisterForm, { onRegistered: () => {} });

    await fireEvent.input(screen.getByPlaceholderText('linux-runner-01'), {
      target: { value: 'dup' },
    });
    await fireEvent.click(screen.getAllByText('admin.runners.register')[1]);

    expect(await screen.findByText('name taken')).toBeInTheDocument();
  });
});
