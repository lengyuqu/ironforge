import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// The real toast store is used: push messages, render the container,
// assert the stack and dismissal. Durations use real timers (≥3s), far
// beyond test runtime.

import ToastContainer from './ToastContainer.svelte';
import { toast } from './toast.svelte';

describe('ToastContainer.svelte', () => {
  beforeEach(() => {
    toast.clear();
  });

  it('renders one toast per queued message with its type icon', () => {
    toast.success('Saved!');
    toast.error('Broken');
    toast.warning('Careful');
    render(ToastContainer);

    const toasts = document.querySelectorAll('.toast');
    expect(toasts.length).toBe(3);
    expect(screen.getByText('Saved!').closest('.toast')?.className).toContain('toast-success');
    expect(screen.getByText('Broken').closest('.toast')?.className).toContain('toast-error');
    expect(screen.getByText('Careful').closest('.toast')?.className).toContain('toast-warning');
    expect(document.querySelector('.toast-success .toast-icon')?.textContent).toContain('✓');
    expect(document.querySelector('.toast-error .toast-icon')?.textContent).toContain('✕');
  });

  it('dismisses individual toasts through the × button', () => {
    toast.info('Sticky note');
    render(ToastContainer);

    expect(screen.getByText('Sticky note')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Dismiss'));
    expect(screen.queryByText('Sticky note')).not.toBeInTheDocument();
  });

  it('stacks multiple toasts at increasing offsets', () => {
    toast.info('One');
    toast.info('Two');
    render(ToastContainer);

    // Offsets are emitted as unevaluated calc() expressions.
    const toasts = Array.from(document.querySelectorAll('.toast'));
    expect(toasts[0].getAttribute('style')).toContain('0 * 64px');
    expect(toasts[1].getAttribute('style')).toContain('1 * 64px');
  });
});
