import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

import ModalHarness from './ModalHarness.svelte';

describe('Modal.svelte (via ModalHarness)', () => {
  it('renders nothing while closed, and the full frame while open', async () => {
    const { rerender } = render(ModalHarness, { open: false });
    expect(document.querySelector('.modal-backdrop')).not.toBeInTheDocument();

    rerender({ open: true, title: 'Confirm' });
    await waitFor(() => expect(document.querySelector('.modal-backdrop')).toBeInTheDocument());
    expect(screen.getByText('Confirm')).toBeInTheDocument();
    expect(screen.getByText('Body content')).toBeInTheDocument();
    expect(screen.getByLabelText('Close')).toBeInTheDocument();
  });

  it('closes through the X button, backdrop click and Escape key', async () => {
    // Only the close pathway matters here (open state handled by the parent).
    const onClose = vi.fn();
    const { rerender } = render(ModalHarness, { open: true, title: 'T', onClose });

    rerender({ open: true, title: 'T', closeOnBackdrop: true, onClose });
    await waitFor(() => expect(document.querySelector('.modal-backdrop')).toBeInTheDocument());
    fireEvent.click(document.querySelector('.modal-backdrop') as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByLabelText('Close'));
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  it('ignores Escape and backdrop clicks when both are disabled', () => {
    const onClose = vi.fn();
    render(ModalHarness, {
      open: true,
      title: 'T',
      closeOnEsc: false,
      closeOnBackdrop: false,
      onClose,
    });

    fireEvent.keyDown(window, { key: 'Escape' });
    fireEvent.click(document.querySelector('.modal-backdrop') as HTMLElement);
    // Still open — close decisions are delegated to the parent only via
    // the X button, which remains available.
    expect(onClose).not.toHaveBeenCalled();
    expect(document.querySelector('.modal-backdrop')).toBeInTheDocument();
    expect(screen.getByLabelText('Close')).toBeInTheDocument();
  });

  it('renders the footer with cancel and submit when onSubmit is given', () => {
    render(ModalHarness, {
      open: true,
      title: 'Delete?',
      submitLabel: 'Delete',
      submitVariant: 'danger',
      onSubmit: () => {},
    });

    const submit = screen.getByText('Delete');
    expect(submit.className).toContain('btn-danger');
    expect(screen.getByText('Cancel')).toBeInTheDocument();
  });

  it('locks body scroll while open and restores it when closed', async () => {
    const { rerender } = render(ModalHarness, { open: true });
    await waitFor(() => expect(document.body.style.overflow).toBe('hidden'));

    rerender({ open: false });
    await waitFor(() => expect(document.body.style.overflow).not.toBe('hidden'));
  });

  it('does not react to Escape while closed', () => {
    const onClose = vi.fn();
    render(ModalHarness, { open: false, onClose });
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });

  it('moves focus into the dialog, traps Tab, and restores focus on close', async () => {
    const opener = document.createElement('button');
    document.body.appendChild(opener);
    opener.focus();

    const { rerender } = render(ModalHarness, { open: false });
    rerender({ open: true, title: 'T', onSubmit: () => {} });
    await waitFor(() => expect(document.querySelector('.modal-content')?.contains(document.activeElement)).toBe(true));

    // Tab from the last control wraps to the first one.
    const items = Array.from(
      document.querySelectorAll<HTMLElement>('.modal-content button'),
    );
    items[items.length - 1].focus();
    fireEvent.keyDown(window, { key: 'Tab' });
    expect(document.activeElement).toBe(items[0]);

    rerender({ open: false });
    await waitFor(() => expect(document.activeElement).toBe(opener));
    opener.remove();
  });

  it('submits on Enter inside a text input but not when disabled', async () => {
    const onSubmit = vi.fn();
    const { rerender } = render(ModalHarness, { open: true, title: 'T', onSubmit, withInput: true });
    const input = await waitFor(() => {
      const el = document.querySelector<HTMLInputElement>('#harness-input');
      expect(el).toBeInTheDocument();
      return el as HTMLInputElement;
    });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSubmit).toHaveBeenCalledTimes(1);

    rerender({ open: true, title: 'T', onSubmit, withInput: true, submitDisabled: true });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it('keeps the dialog open when a text-selection drag ends on the backdrop', async () => {
    const onClose = vi.fn();
    render(ModalHarness, { open: true, title: 'T', onClose });
    const backdrop = document.querySelector('.modal-backdrop') as HTMLElement;
    await fireEvent.mouseDown(document.querySelector('.modal-content') as HTMLElement);
    await fireEvent.click(backdrop);
    expect(onClose).not.toHaveBeenCalled();
  });
});
