import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const addMock = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  timeTracking: { add: (...a: unknown[]) => addMock(...a) },
}));

import TimeEntryForm from './TimeEntryForm.svelte';

describe('TimeEntryForm.svelte', () => {
  beforeEach(() => {
    addMock.mockReset();
    addMock.mockResolvedValue(undefined);
  });

  it('adds an entry with rounded minutes and the description, then resets', async () => {
    const onAdded = vi.fn().mockResolvedValue(undefined);
    render(TimeEntryForm, { owner: 'acme', repo: 'web', issueNumber: 7, onAdded });

    await fireEvent.input(screen.getByLabelText('Duration (hours)'), { target: { value: '2.5' } });
    await fireEvent.input(screen.getByLabelText('Note'), { target: { value: 'work' } });
    await fireEvent.click(screen.getByText('Add'));

    await waitFor(() =>
      expect(addMock).toHaveBeenCalledWith('acme', 'web', 7, {
        duration_minutes: 150,
        description: 'work',
      })
    );
    await waitFor(() => expect(onAdded).toHaveBeenCalledTimes(1));

    expect((screen.getByLabelText('Duration (hours)') as HTMLInputElement).value).toBe('1');
    expect((screen.getByLabelText('Note') as HTMLInputElement).value).toBe('');
  });

  it('sends undefined description when the note is empty', async () => {
    const onAdded = vi.fn().mockResolvedValue(undefined);
    render(TimeEntryForm, { owner: 'acme', repo: 'web', issueNumber: 7, onAdded });

    await fireEvent.input(screen.getByLabelText('Duration (hours)'), { target: { value: '1' } });
    await fireEvent.click(screen.getByText('Add'));

    await waitFor(() =>
      expect(addMock).toHaveBeenCalledWith('acme', 'web', 7, {
        duration_minutes: 60,
        description: undefined,
      })
    );
    await waitFor(() => expect(onAdded).toHaveBeenCalledTimes(1));
  });

  it('disables the add button when duration is zero or less', async () => {
    render(TimeEntryForm, { owner: 'acme', repo: 'web', issueNumber: 7, onAdded: vi.fn() });

    await fireEvent.input(screen.getByLabelText('Duration (hours)'), { target: { value: '0' } });
    const add = screen.getByText('Add').closest('button') as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    expect(addMock).not.toHaveBeenCalled();
  });

  it('shows an error banner and does not call onAdded when add fails', async () => {
    addMock.mockRejectedValue(new Error('boom'));
    const onAdded = vi.fn().mockResolvedValue(undefined);
    render(TimeEntryForm, { owner: 'acme', repo: 'web', issueNumber: 7, onAdded });

    await fireEvent.input(screen.getByLabelText('Duration (hours)'), { target: { value: '1' } });
    await fireEvent.click(screen.getByText('Add'));

    await waitFor(() => expect(screen.getByText('boom')).toBeInTheDocument());
    expect(onAdded).not.toHaveBeenCalled();
  });
});
