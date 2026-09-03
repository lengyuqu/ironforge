import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

const downloadApiFile = vi.fn();
vi.mock('$lib/api/_base.svelte', () => ({
  withBackendBase: (path: string) => `http://backend.test${path}`,
  buildSshCloneUrl: (owner: string, repo: string, host?: string) =>
    `ssh://git@${host || 'localhost'}:2222/${owner}/${repo}`,
  downloadApiFile: (...a: unknown[]) => downloadApiFile(...a),
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import CloneModal from './CloneModal.svelte';

const writeText = vi.fn();

function renderModal(overrides: Record<string, unknown> = {}) {
  const onClose = vi.fn();
  render(CloneModal, { owner: 'alice', repo: 'demo', open: true, onClose, ...overrides });
  return { onClose };
}

describe('CloneModal.svelte', () => {
  beforeEach(() => {
    downloadApiFile.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
    writeText.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders nothing while closed', () => {
    renderModal({ open: false });

    expect(document.querySelector('.clone-dropdown')).not.toBeInTheDocument();
  });

  it('shows the HTTPS clone url by default and switches to SSH', () => {
    renderModal();

    const input = screen.getByLabelText('repo.clone_url') as HTMLInputElement;
    expect(input.value).toBe('http://backend.test/git/alice/demo');

    fireEvent.click(screen.getByRole('tab', { name: 'SSH clone' }));
    expect(input.value).toBe('ssh://git@localhost:2222/alice/demo');
  });

  it('copies the current url to the clipboard and flashes the check icon', async () => {
    renderModal();

    await fireEvent.click(screen.getByLabelText('repo.copy_clone_url'));
    expect(writeText).toHaveBeenCalledWith('http://backend.test/git/alice/demo');
    expect(document.querySelector('.copied-check')).toBeInTheDocument();
  });

  it('downloads the archive via downloadApiFile and toasts on success', async () => {
    renderModal();

    await fireEvent.click(screen.getByLabelText('repo.download_zip'));
    expect(downloadApiFile).toHaveBeenCalledWith(
      '/repos/alice/demo/archive/main.zip',
      'demo-main.zip'
    );
    expect(toastSuccess).toHaveBeenCalledWith('Archive download started');
  });

  it('closes on Escape and backdrop clicks', () => {
    const { onClose } = renderModal();

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.click(document.querySelector('.clone-backdrop') as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
