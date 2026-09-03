import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const mirrorCreate = vi.fn();
const mirrorUpdate = vi.fn();
const mirrorSync = vi.fn();
const mirrorRemove = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  mirrors: {
    create: (...a: unknown[]) => mirrorCreate(...a),
    update: (...a: unknown[]) => mirrorUpdate(...a),
    sync: (...a: unknown[]) => mirrorSync(...a),
    remove: (...a: unknown[]) => mirrorRemove(...a),
  },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import MirrorForm from './MirrorForm.svelte';
import type { RepositoryMirror } from '$lib/api/mirrors';

const confirmMock = vi.fn(() => true);

function makeMirror(overrides: Partial<RepositoryMirror> = {}): RepositoryMirror {
  return {
    id: 1,
    repo_id: 10,
    url: 'https://upstream.example/demo.git',
    username: 'ci-bot',
    sync_interval_seconds: 7200, // 2h
    next_sync_at: null,
    last_sync_at: null,
    last_sync_error: null,
    status: 'idle',
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function renderForm(mirror: RepositoryMirror | null = null) {
  const onChanged = vi.fn().mockResolvedValue(undefined);
  render(MirrorForm, { owner: 'alice', repo: 'demo', mirror, onChanged });
  return { onChanged };
}

describe('MirrorForm.svelte', () => {
  beforeEach(() => {
    mirrorCreate.mockReset().mockResolvedValue(makeMirror());
    mirrorUpdate.mockReset().mockResolvedValue(makeMirror());
    mirrorSync.mockReset().mockResolvedValue(undefined);
    mirrorRemove.mockReset().mockResolvedValue(undefined);
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('create mode: renders the create title without sync/delete actions', () => {
    renderForm(null);

    expect(screen.getByText('settings.mirror.create_title')).toBeInTheDocument();
    expect(screen.queryByText('settings.mirror.sync_now')).not.toBeInTheDocument();
    expect(screen.queryByText('settings.mirror.delete')).not.toBeInTheDocument();
    // Save stays disabled until a URL is entered.
    expect(screen.getByText('common.save')).toBeDisabled();
  });

  it('create mode: submits trimmed fields with interval converted to seconds', async () => {
    const { onChanged } = renderForm(null);

    fireEvent.input(screen.getByLabelText('settings.mirror.url'), {
      target: { value: '  https://git.example/x.git  ' },
    });
    fireEvent.input(screen.getByLabelText('settings.mirror.username'), {
      target: { value: ' bot ' },
    });
    fireEvent.input(screen.getByLabelText('settings.mirror.password'), {
      target: { value: ' secret ' },
    });
    fireEvent.input(screen.getByLabelText('settings.mirror.interval_hours'), {
      target: { value: '6' },
    });
    await fireEvent.submit(screen.getByRole('button', { name: 'common.save' }).closest('form')!);

    await waitFor(() =>
      expect(mirrorCreate).toHaveBeenCalledWith('alice', 'demo', {
        url: 'https://git.example/x.git',
        username: 'bot',
        password: 'secret',
        sync_interval_seconds: 21600,
      })
    );
    expect(screen.getByText('settings.mirror.created')).toBeInTheDocument();
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('edit mode: prefills url/username/interval from the mirror, password blank', () => {
    renderForm(makeMirror());

    expect(screen.getByText('settings.mirror.edit_title')).toBeInTheDocument();
    expect(screen.getByLabelText('settings.mirror.url')).toHaveValue(
      'https://upstream.example/demo.git'
    );
    expect(screen.getByLabelText('settings.mirror.username')).toHaveValue('ci-bot');
    expect(screen.getByLabelText('settings.mirror.password')).toHaveValue('');
    // 7200s → 2h
    expect(screen.getByLabelText('settings.mirror.interval_hours')).toHaveValue(2);
    expect(screen.getByText('settings.mirror.sync_now')).toBeInTheDocument();
    expect(screen.getByText('settings.mirror.delete')).toBeInTheDocument();
  });

  it('edit mode: save routes through mirrors.update and refreshes', async () => {
    const { onChanged } = renderForm(makeMirror());

    await fireEvent.submit(screen.getByRole('button', { name: 'common.save' }).closest('form')!);

    await waitFor(() => expect(mirrorUpdate).toHaveBeenCalledTimes(1));
    expect(mirrorCreate).not.toHaveBeenCalled();
    expect(screen.getByText('settings.mirror.updated')).toBeInTheDocument();
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('syncs the mirror on demand, surfacing failures inline', async () => {
    mirrorSync.mockRejectedValueOnce(new Error('upstream unreachable'));
    const { onChanged } = renderForm(makeMirror());

    await fireEvent.click(screen.getByText('settings.mirror.sync_now'));
    await waitFor(() => expect(mirrorSync).toHaveBeenCalledWith('alice', 'demo'));
    expect(screen.getByText('upstream unreachable')).toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it('removes the mirror after confirmation and refreshes', async () => {
    const { onChanged } = renderForm(makeMirror());

    await fireEvent.click(screen.getByText('settings.mirror.delete'));
    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() => expect(mirrorRemove).toHaveBeenCalledWith('alice', 'demo'));
    expect(screen.getByText('settings.mirror.deleted')).toBeInTheDocument();
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('save failure shows the error box and never refreshes', async () => {
    mirrorCreate.mockRejectedValueOnce(new Error('invalid url'));
    const { onChanged } = renderForm(null);

    fireEvent.input(screen.getByLabelText('settings.mirror.url'), {
      target: { value: 'https://git.example/x.git' },
    });
    await fireEvent.submit(screen.getByRole('button', { name: 'common.save' }).closest('form')!);

    await waitFor(() => expect(screen.getByText('invalid url')).toBeInTheDocument());
    expect(onChanged).not.toHaveBeenCalled();
  });
});
