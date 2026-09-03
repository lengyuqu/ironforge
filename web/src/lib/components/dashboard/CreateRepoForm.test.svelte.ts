import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const reposCreate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  repos: {
    create: (...a: unknown[]) => reposCreate(...a),
    templates: {
      gitignores: () => Promise.resolve({ data: [{ key: 'rust', name: 'Rust', description: '' }] }),
      licenses: () => Promise.resolve({ data: [{ key: 'mit', name: 'MIT', description: '' }] }),
      readmes: () => Promise.resolve({ data: [{ key: 'default', name: 'Default', description: '' }] }),
      labels: () => Promise.resolve({ data: [{ key: 'default', name: 'Default', description: '' }] }),
    },
  },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a), warning: vi.fn() },
}));

import CreateRepoForm from './CreateRepoForm.svelte';

const nameInput = () =>
  document.querySelector<HTMLInputElement>('input[required]');

describe('CreateRepoForm.svelte', () => {
  beforeEach(() => {
    reposCreate.mockReset();
    toastError.mockClear();
  });

  it('shows an inline error for invalid repo names and blocks submission', async () => {
    render(CreateRepoForm, { owner: 'alice', onCreated: () => {}, onCancel: () => {} });

    await fireEvent.input(nameInput()!, { target: { value: '-bad-name' } });
    expect(await screen.findByText('dashboard.create_form.name_invalid')).toBeInTheDocument();

    // Submit the form event directly; canSubmit is false so nothing is sent.
    await fireEvent.submit(document.querySelector('form')!);
    expect(reposCreate).not.toHaveBeenCalled();
  });

  it('creates a repo with the entered fields and default template choices', async () => {
    reposCreate.mockResolvedValue(undefined);
    const onCreated = vi.fn();
    render(CreateRepoForm, { owner: 'alice', onCreated, onCancel: () => {} });

    await fireEvent.input(nameInput()!, { target: { value: 'my-repo' } });
    await fireEvent.submit(document.querySelector('form')!);

    await waitFor(() =>
      expect(reposCreate).toHaveBeenCalledWith({
        name: 'my-repo',
        description: undefined,
        is_private: false,
        auto_init: true,
        default_branch: 'main',
        gitignores: undefined,
        license: undefined,
        readme: 'default',
        issue_labels: 'default',
      })
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith('my-repo'));
  });

  it('toasts when the API rejects the create', async () => {
    reposCreate.mockRejectedValue(new Error('name exists'));
    render(CreateRepoForm, { owner: 'alice', onCreated: () => {}, onCancel: () => {} });

    await fireEvent.input(nameInput()!, { target: { value: 'my-repo' } });
    await fireEvent.submit(document.querySelector('form')!);

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('name exists'));
  });
});
