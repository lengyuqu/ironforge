import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const importsStart = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  imports: { start: (...a: unknown[]) => importsStart(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import ImportForm from './ImportForm.svelte';

describe('ImportForm.svelte', () => {
  beforeEach(() => {
    importsStart.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('renders the platform picker with metadata options enabled for github', () => {
    render(ImportForm, { initialOwner: 'alice', onStarted: () => {} });

    const platformSelect = document.querySelector('select')!;
    expect(platformSelect.querySelectorAll('option')).toHaveLength(4);

    const checkboxes = Array.from(
      document.querySelectorAll<HTMLInputElement>('fieldset input[type="checkbox"]')
    );
    // Repository / Issues / PRs / Releases / Labels / Milestones default on;
    // Wiki defaults off.
    expect(checkboxes.map((c) => c.checked)).toEqual([true, true, true, false, true, true, true]);
    expect(checkboxes[1].disabled).toBe(false); // metadata import supported
    expect(screen.getByText('Start import')).toBeDisabled();
  });

  it('submits the full payload and resets the source url on success', async () => {
    importsStart.mockResolvedValue(undefined);
    const onStarted = vi.fn();
    render(ImportForm, { initialOwner: 'alice', onStarted });

    await fireEvent.input(screen.getByLabelText('Source repository URL'), {
      target: { value: 'https://github.com/example/project' },
    });
    await fireEvent.input(screen.getByLabelText('Target repository'), {
      target: { value: 'renamed' },
    });
    await fireEvent.input(screen.getByLabelText(/Source access token/), {
      target: { value: 'tok123' },
    });
    await fireEvent.submit(document.querySelector('form')!);

    await waitFor(() =>
      expect(importsStart).toHaveBeenCalledWith({
        platform: 'github',
        source_url: 'https://github.com/example/project',
        target_owner: 'alice',
        import_repo: true,
        import_issues: true,
        import_pull_requests: true,
        import_wiki: false,
        import_releases: true,
        import_labels: true,
        import_milestones: true,
        target_name: 'renamed',
        auth_token: 'tok123',
      })
    );
    await waitFor(() => expect(onStarted).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Import queued'));
    expect(
      (screen.getByLabelText('Source repository URL') as HTMLInputElement).value
    ).toBe('');
  });

  it('toasts when the import cannot be started', async () => {
    importsStart.mockRejectedValue(new Error('invalid url'));
    render(ImportForm, { initialOwner: 'alice', onStarted: () => {} });

    await fireEvent.input(screen.getByLabelText('Source repository URL'), {
      target: { value: 'https://github.com/example/project' },
    });
    await fireEvent.submit(document.querySelector('form')!);

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('invalid url'));
  });
});
