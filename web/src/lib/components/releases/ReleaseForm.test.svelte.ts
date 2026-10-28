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

  describe('existing-tag hints', () => {
    const manyTags = Array.from({ length: 12 }, (_, i) => `v${i}`);

    it('shows at most ten hint buttons and counts the rest', () => {
      render(ReleaseForm, { owner: 'alice', repo: 'demo', tags: manyTags });

      const hints = document.querySelectorAll('.tag-hint');
      expect(hints).toHaveLength(10);
      expect(hints[0].textContent?.trim()).toBe('v0');
      expect(hints[9].textContent?.trim()).toBe('v9');
      expect(screen.getByText('releases.more_tags')).toBeInTheDocument();
    });

    it('omits the "more" note when ten or fewer tags exist', () => {
      render(ReleaseForm, { owner: 'alice', repo: 'demo', tags: manyTags.slice(0, 10) });
      expect(document.querySelectorAll('.tag-hint')).toHaveLength(10);
      expect(screen.queryByText('releases.more_tags')).toBeNull();
    });

    it('renders no hints without tags, and none in edit mode', () => {
      const { unmount } = render(ReleaseForm, { owner: 'alice', repo: 'demo' });
      expect(document.querySelector('.tag-hints')).toBeNull();
      unmount();

      render(ReleaseForm, {
        owner: 'alice',
        repo: 'demo',
        tags: ['v1'],
        release: { id: 1, tag_name: 'v1', title: 't' } as unknown as Release,
      });
      expect(document.querySelector('.tag-hints')).toBeNull();
    });
  });

  describe('target picker', () => {
    const targetSelect = () => document.getElementById('target-commitish') as HTMLSelectElement;
    const optionValues = () => Array.from(targetSelect().options).map((o) => o.value);

    it('starts on tags and lists them after the empty option', () => {
      render(ReleaseForm, { owner: 'a', repo: 'r', tags: ['v1', 'v2'], branches: ['main', 'dev'] });
      expect(optionValues()).toEqual(['', 'v1', 'v2']);
      expect(screen.getByText('releases.target_tags')).toHaveClass('active');
    });

    it('switches to branches and back', async () => {
      render(ReleaseForm, { owner: 'a', repo: 'r', tags: ['v1'], branches: ['main', 'dev'] });

      await fireEvent.click(screen.getByText('releases.target_branches'));
      expect(optionValues()).toEqual(['', 'main', 'dev']);
      expect(screen.getByText('releases.target_branches')).toHaveClass('active');
      expect(screen.getByText('releases.target_tags')).not.toHaveClass('active');

      await fireEvent.click(screen.getByText('releases.target_tags'));
      expect(optionValues()).toEqual(['', 'v1']);
    });

    it('sends the chosen branch as target_commitish', async () => {
      releasesCreate.mockResolvedValue(undefined);
      render(ReleaseForm, { owner: 'a', repo: 'r', branches: ['main', 'dev'], onCreated: () => {} });

      await fireEvent.input(screen.getByLabelText(/tag_name/), { target: { value: 'v9' } });
      await fireEvent.input(screen.getByLabelText(/release_title/), { target: { value: 'T' } });
      await fireEvent.click(screen.getByText('releases.target_branches'));
      // happy-dom has no `:checked` for <option>, which Svelte's select binding
      // queries to read the choice; answer it from selectedIndex for this test.
      const original = HTMLSelectElement.prototype.querySelector;
      const spy = vi
        .spyOn(HTMLSelectElement.prototype, 'querySelector')
        .mockImplementation(function (this: HTMLSelectElement, selector: string) {
          return selector === ':checked'
            ? (this.options[this.selectedIndex] ?? null)
            : original.call(this, selector);
        });
      try {
        await fireEvent.change(targetSelect(), { target: { value: 'dev' } });
      } finally {
        spy.mockRestore();
      }
      await fireEvent.click(submitButton()!);

      await waitFor(() => expect(releasesCreate).toHaveBeenCalledTimes(1));
      expect(releasesCreate.mock.calls[0][2]).toMatchObject({ tag_name: 'v9', target_commitish: 'dev' });
    });

    it('is absent in edit mode', () => {
      render(ReleaseForm, {
        owner: 'a',
        repo: 'r',
        release: { id: 1, tag_name: 'v1', title: 't' } as unknown as Release,
      });
      expect(document.getElementById('target-commitish')).toBeNull();
    });
  });

  describe('flags and submit state', () => {
    it('sends draft and pre-release flags on create', async () => {
      releasesCreate.mockResolvedValue(undefined);
      render(ReleaseForm, { owner: 'a', repo: 'r', onCreated: () => {} });
      await fireEvent.input(screen.getByLabelText(/tag_name/), { target: { value: 'v1' } });
      await fireEvent.input(screen.getByLabelText(/release_title/), { target: { value: 'T' } });
      await fireEvent.click(screen.getByText('releases.is_draft'));
      await fireEvent.click(screen.getByText('releases.is_prerelease'));
      await fireEvent.click(submitButton()!);

      await waitFor(() => expect(releasesCreate).toHaveBeenCalledTimes(1));
      expect(releasesCreate.mock.calls[0][2]).toMatchObject({ is_draft: true, is_prerelease: true });
    });

    it('edit mode preloads the existing flags', () => {
      render(ReleaseForm, {
        owner: 'a',
        repo: 'r',
        release: { id: 1, tag_name: 'v1', title: 't', is_draft: true, is_prerelease: false } as unknown as Release,
      });
      const [draft, pre] = Array.from(document.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'));
      expect(draft.checked).toBe(true);
      expect(pre.checked).toBe(false);
    });

    it('omits an empty body', async () => {
      releasesCreate.mockResolvedValue(undefined);
      render(ReleaseForm, { owner: 'a', repo: 'r', onCreated: () => {} });
      await fireEvent.input(screen.getByLabelText(/tag_name/), { target: { value: 'v1' } });
      await fireEvent.input(screen.getByLabelText(/release_title/), { target: { value: 'T' } });
      await fireEvent.input(screen.getByLabelText('releases.body'), { target: { value: '   ' } });
      await fireEvent.click(submitButton()!);

      await waitFor(() => expect(releasesCreate).toHaveBeenCalledTimes(1));
      expect(releasesCreate.mock.calls[0][2].body).toBeUndefined();
    });

    it('disables submit and shows the progress label while saving', async () => {
      let resolve!: () => void;
      releasesCreate.mockReturnValue(new Promise<void>((r) => (resolve = r)));
      render(ReleaseForm, { owner: 'a', repo: 'r', onCreated: () => {} });
      expect(submitButton()).toHaveTextContent('releases.submit');

      await fireEvent.input(screen.getByLabelText(/tag_name/), { target: { value: 'v1' } });
      await fireEvent.input(screen.getByLabelText(/release_title/), { target: { value: 'T' } });
      await fireEvent.click(submitButton()!);

      await waitFor(() => expect(submitButton()).toBeDisabled());
      expect(submitButton()).toHaveTextContent('releases.submitting');
      resolve();
    });

    it('re-enables submit after a failed request', async () => {
      releasesCreate.mockRejectedValue(new Error('boom'));
      render(ReleaseForm, { owner: 'a', repo: 'r', onCreated: () => {} });
      await fireEvent.input(screen.getByLabelText(/tag_name/), { target: { value: 'v1' } });
      await fireEvent.input(screen.getByLabelText(/release_title/), { target: { value: 'T' } });
      await fireEvent.click(submitButton()!);

      await waitFor(() => expect(toastError).toHaveBeenCalled());
      expect(submitButton()).toBeEnabled();
    });

    it('links Cancel back to the releases list', () => {
      render(ReleaseForm, { owner: 'alice', repo: 'demo' });
      expect(screen.getByText('common.cancel')).toHaveAttribute('href', '/alice/demo/releases');
    });
  });
});
