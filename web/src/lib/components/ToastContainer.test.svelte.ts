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

  it('stacks multiple toasts in one flex container instead of fixed offsets', () => {
    toast.info('One');
    toast.info('Two');
    render(ToastContainer);

    const stack = document.querySelector('.toast-stack') as HTMLElement;
    expect(stack).toBeInTheDocument();
    expect(Array.from(stack.children).map((el) => el.textContent?.includes('One') || el.textContent?.includes('Two'))).toEqual([true, true]);
  });

  it('announces errors with role=alert and keeps at most five toasts', () => {
    for (let i = 0; i < 7; i++) toast.info(`n${i}`);
    toast.error('Boom');
    render(ToastContainer);

    expect(document.querySelectorAll('.toast').length).toBe(5);
    expect(screen.getByRole('alert')).toHaveTextContent('Boom');
    expect(screen.queryByText('n0')).not.toBeInTheDocument();
  });
});
