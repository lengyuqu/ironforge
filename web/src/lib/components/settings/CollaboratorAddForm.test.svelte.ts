import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const collaboratorsAdd = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  collaborators: { add: (...a: unknown[]) => collaboratorsAdd(...a) },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import CollaboratorAddForm from './CollaboratorAddForm.svelte';

function renderForm() {
  const onAdded = vi.fn().mockResolvedValue(undefined);
  render(CollaboratorAddForm, { owner: 'alice', repo: 'demo', onAdded });
  return { onAdded };
}

describe('CollaboratorAddForm.svelte', () => {
  beforeEach(() => {
    collaboratorsAdd.mockReset().mockResolvedValue(undefined);
  });

  it('keeps the add button disabled until an identifier is entered', () => {
    renderForm();

    expect(screen.getByText('settings.collaborators.add')).toBeDisabled();
    fireEvent.input(screen.getByLabelText('settings.collaborators.user_identifier'), {
      target: { value: 'bob' },
    });
    expect(screen.getByText('settings.collaborators.add')).toBeEnabled();
  });

  it('adds a collaborator with the trimmed identifier and default permission', async () => {
    const { onAdded } = renderForm();

    fireEvent.input(screen.getByLabelText('settings.collaborators.user_identifier'), {
      target: { value: '  bob  ' },
    });
    await fireEvent.submit(screen.getByText('settings.collaborators.add').closest('form')!);

    await waitFor(() =>
      expect(collaboratorsAdd).toHaveBeenCalledWith('alice', 'demo', 'bob', 'read')
    );
    // Form resets after success.
    expect(screen.getByLabelText('settings.collaborators.user_identifier')).toHaveValue('');
    await waitFor(() => expect(onAdded).toHaveBeenCalledTimes(1));
  });

  it('blocks empty identifiers with an inline error and no API call', async () => {
    const { onAdded } = renderForm();

    fireEvent.input(screen.getByLabelText('settings.collaborators.user_identifier'), {
      target: { value: '   ' },
    });
    await fireEvent.submit(screen.getByText('settings.collaborators.add').closest('form')!);

    expect(screen.getByText('settings.collaborators.user_required')).toBeInTheDocument();
    expect(collaboratorsAdd).not.toHaveBeenCalled();
    expect(onAdded).not.toHaveBeenCalled();
  });

  it('surfaces add failures inline', async () => {
    collaboratorsAdd.mockRejectedValueOnce(new Error('user not found'));
    renderForm();

    fireEvent.input(screen.getByLabelText('settings.collaborators.user_identifier'), {
      target: { value: 'ghost' },
    });
    await fireEvent.submit(screen.getByText('settings.collaborators.add').closest('form')!);

    await waitFor(() => expect(screen.getByText('user not found')).toBeInTheDocument());
  });
});
