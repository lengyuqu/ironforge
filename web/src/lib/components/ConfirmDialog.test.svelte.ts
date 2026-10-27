import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

import ConfirmDialog from './ConfirmDialog.svelte';
import { confirmDialog, promptDialog } from './confirm.svelte';

describe('ConfirmDialog.svelte', () => {
  it('resolves true when confirmed and false when cancelled', async () => {
    render(ConfirmDialog);

    const yes = confirmDialog({ message: 'Really?', variant: 'danger' });
    await waitFor(() => expect(screen.getByText('Really?')).toBeInTheDocument());
    // Destructive dialogs start on Cancel.
    expect(document.activeElement?.textContent).toBe('Cancel');
    await fireEvent.click(screen.getByText('Delete'));
    expect(await yes).toBe(true);
    await waitFor(() => expect(screen.queryByText('Really?')).not.toBeInTheDocument());

    const no = confirmDialog({ message: 'Again?' });
    await waitFor(() => expect(screen.getByText('Again?')).toBeInTheDocument());
    await fireEvent.click(screen.getByText('Cancel'));
    expect(await no).toBe(false);
  });

  it('resolves false on Escape', async () => {
    render(ConfirmDialog);
    const result = confirmDialog({ message: 'Esc me' });
    await waitFor(() => expect(screen.getByText('Esc me')).toBeInTheDocument());
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(await result).toBe(false);
  });

  it('queues concurrent requests and answers them in order', async () => {
    render(ConfirmDialog);
    const first = confirmDialog({ message: 'First' });
    const second = confirmDialog({ message: 'Second' });

    await waitFor(() => expect(screen.getByText('First')).toBeInTheDocument());
    expect(screen.queryByText('Second')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByText('Confirm'));
    expect(await first).toBe(true);

    await waitFor(() => expect(screen.getByText('Second')).toBeInTheDocument());
    await fireEvent.click(screen.getByText('Cancel'));
    expect(await second).toBe(false);
  });

  it('prompt requires input when asked and returns the trimmed text; cancel gives null', async () => {
    render(ConfirmDialog);
    const reason = promptDialog({ message: 'Why?', required: true, confirmLabel: 'Send' });
    await waitFor(() => expect(screen.getByText('Why?')).toBeInTheDocument());

    const send = screen.getByText('Send') as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    const input = document.querySelector('.confirm-input') as HTMLInputElement;
    await waitFor(() => expect(document.activeElement).toBe(input));
    await fireEvent.input(input, { target: { value: '  because  ' } });
    expect(send.disabled).toBe(false);
    await fireEvent.click(send);
    expect(await reason).toBe('because');

    const cancelled = promptDialog({ message: 'Again?' });
    await waitFor(() => expect(screen.getByText('Again?')).toBeInTheDocument());
    await fireEvent.click(screen.getByText('Cancel'));
    expect(await cancelled).toBeNull();
  });
});
