import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const labelsCreate = vi.fn();
const labelsUpdate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  labels: {
    create: (...a: unknown[]) => labelsCreate(...a),
    update: (...a: unknown[]) => labelsUpdate(...a),
  },
}));

const toastSuccess = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: vi.fn(),
  },
}));

import LabelFormModal from './LabelFormModal.svelte';
import type { Label } from '$lib/types/entities';

describe('LabelFormModal.svelte', () => {
  beforeEach(() => {
    labelsCreate.mockReset();
    labelsUpdate.mockReset();
    toastSuccess.mockClear();
  });

  it('creates a label with trimmed name and empty description as undefined', async () => {
    labelsCreate.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onSaved = vi.fn();
    render(LabelFormModal, { owner: 'alice', repo: 'demo', label: null, onClose, onSaved });

    expect(screen.getByText('settings.new_label')).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText('settings.label_name'), {
      target: { value: '  enhancement ' },
    });
    // Pick the second preset swatch (#00ff00).
    await fireEvent.click(screen.getByLabelText('Color #00ff00'));

    await fireEvent.click(screen.getByText('settings.create_label'));
    await waitFor(() =>
      expect(labelsCreate).toHaveBeenCalledWith('alice', 'demo', 'enhancement', '#00ff00', undefined)
    );
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(onSaved).toHaveBeenCalledTimes(1);
    expect(toastSuccess).toHaveBeenCalledWith('Label created');
  });

  it('updates an existing label from prefilled values', async () => {
    labelsUpdate.mockResolvedValue(undefined);
    const label = { id: 9, name: 'bug', color: '#ff0000', description: 'old' } as Label;
    const onClose = vi.fn();
    render(LabelFormModal, { owner: 'alice', repo: 'demo', label, onClose, onSaved: () => {} });

    expect(screen.getByText('settings.edit_label')).toBeInTheDocument();
    expect((screen.getByLabelText('settings.label_name') as HTMLInputElement).value).toBe('bug');
    expect((screen.getByLabelText('settings.label_color') as HTMLInputElement).value).toBe(
      '#ff0000'
    );

    await fireEvent.input(screen.getByLabelText('settings.label_desc'), {
      target: { value: '  something broke ' },
    });
    await fireEvent.click(screen.getByText('settings.save_label'));

    await waitFor(() =>
      expect(labelsUpdate).toHaveBeenCalledWith('alice', 'demo', 9, {
        name: 'bug',
        color: '#ff0000',
        description: 'something broke',
      })
    );
  });

  it('blocks submission when the name is blank', async () => {
    render(LabelFormModal, { owner: 'alice', repo: 'demo', label: null, onClose: () => {}, onSaved: () => {} });

    await fireEvent.click(screen.getByText('settings.create_label'));
    expect(await screen.findByText('Label name is required')).toBeInTheDocument();
    expect(labelsCreate).not.toHaveBeenCalled();
  });

  it('shows an inline error when the API rejects the save', async () => {
    labelsCreate.mockRejectedValue(new Error('duplicate'));
    render(LabelFormModal, { owner: 'alice', repo: 'demo', label: null, onClose: () => {}, onSaved: () => {} });

    await fireEvent.input(screen.getByLabelText('settings.label_name'), {
      target: { value: 'bug' },
    });
    await fireEvent.click(screen.getByText('settings.create_label'));

    expect(await screen.findByText('duplicate')).toBeInTheDocument();
  });
});
