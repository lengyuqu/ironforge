import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const releasesCreate = vi.fn();
const releasesUpdate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  releases: {
    create: (...a: unknown[]) => releasesCreate(...a),
    update: (...a: unknown[]) => releasesUpdate(...a),
  },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import ReleaseForm from './ReleaseForm.svelte';
import type { Release } from '$lib/types/entities';

function submitButton() {
  return document.querySelector<HTMLButtonElement>('button[type="submit"]');
}

describe('ReleaseForm.svelte', () => {
  beforeEach(() => {
    releasesCreate.mockReset();
    releasesUpdate.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('creates a release with trimmed fields and calls onCreated', async () => {
    releasesCreate.mockResolvedValue(undefined);
    const onCreated = vi.fn();
    render(ReleaseForm, {
      owner: 'alice',
      repo: 'demo',
      branches: ['main'],
      tags: ['v1.0.0'],
      onCreated,
    });

    // Tag hints are rendered in create mode (the same text also exists as a
    // select option, so scope the query to the hint button).
    expect(screen.getByText('v1.0.0', { selector: 'button' })).toBeInTheDocument();

    // The tag text also appears in the target select options; scope to the hint button.
    await fireEvent.click(screen.getByText('v1.0.0', { selector: 'button' }));
    expect((screen.getByLabelText(/tag_name/) as HTMLInputElement).value).toBe('v1.0.0');

    await fireEvent.input(screen.getByLabelText(/release_title/), {
      target: { value: '  Big Release ' },
    });
    await fireEvent.input(screen.getByLabelText('releases.body'), {
      target: { value: 'notes' },
    });
    await fireEvent.click(screen.getByText('releases.is_prerelease'));

    await fireEvent.click(submitButton()!);
    await waitFor(() =>
      expect(releasesCreate).toHaveBeenCalledWith('alice', 'demo', {
        tag_name: 'v1.0.0',
        title: 'Big Release',
        body: 'notes',
        target_commitish: undefined,
        is_draft: false,
        is_prerelease: true,
      })
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));
  });

  it('edit mode locks the tag input and updates without tag fields', async () => {
    releasesUpdate.mockResolvedValue(undefined);
    const release = {
      id: 5,
      tag_name: 'v2.0.0',
      title: 'v2',
      body: 'old',
      is_draft: false,
      is_prerelease: false,
    } as unknown as Release;
    const onSaved = vi.fn();
    render(ReleaseForm, { owner: 'alice', repo: 'demo', release, onSaved });

    const tagInput = screen.getByLabelText(/tag_name/) as HTMLInputElement;
    expect(tagInput.disabled).toBe(true);
    expect(tagInput.value).toBe('v2.0.0');
    // No target picker in edit mode.
    expect(screen.queryByLabelText(/target_commitish|Target/i)).toBeNull();

    await fireEvent.click(submitButton()!);
    await waitFor(() =>
      expect(releasesUpdate).toHaveBeenCalledWith('alice', 'demo', 5, {
        title: 'v2',
        body: 'old',
        is_draft: false,
        is_prerelease: false,
      })
    );
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(toastSuccess).toHaveBeenCalledWith('Release updated');
  });

  it('validates required fields before submitting', async () => {
    render(ReleaseForm, { owner: 'alice', repo: 'demo' });

    // Native validation blocks submit for empty required inputs; dispatch
    // the submit event directly to exercise the JS-side guard clauses.
    await fireEvent.submit(document.querySelector('form')!);
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('Tag name is required'));

    await fireEvent.input(screen.getByLabelText(/tag_name/), { target: { value: 'v3' } });
    await fireEvent.submit(document.querySelector('form')!);
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('Release title is required'));
    expect(releasesCreate).not.toHaveBeenCalled();
  });

  it('toasts when the API rejects the submission', async () => {
    releasesCreate.mockRejectedValue(new Error('tag exists'));
    render(ReleaseForm, { owner: 'alice', repo: 'demo', onCreated: () => {} });

    await fireEvent.input(screen.getByLabelText(/tag_name/), { target: { value: 'v3' } });
    await fireEvent.input(screen.getByLabelText(/release_title/), { target: { value: 'T' } });
    await fireEvent.click(submitButton()!);

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('tag exists'));
  });
});
