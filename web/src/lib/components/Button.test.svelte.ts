import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// Snippet props can't be created by @testing-library/svelte directly,
// so ButtonHarness forwards plain props into <Button> with children.

import ButtonHarness from './ButtonHarness.svelte';

describe('Button.svelte (via ButtonHarness)', () => {
  it('renders a button with variant/size class composition and a custom class', () => {
    render(ButtonHarness, { label: 'Click', variant: 'primary', size: 'lg', class: 'extra' });

    const btn = screen.getByText('Click').closest('button') as HTMLButtonElement;
    expect(btn.className).toContain('btn-primary');
    expect(btn.className).toContain('btn-lg');
    expect(btn.className).toContain('extra');
  });

  it('defaults to an outline medium button and forwards clicks', () => {
    const onclick = vi.fn();
    render(ButtonHarness, { label: 'Go', onclick });

    const btn = screen.getByText('Go').closest('button') as HTMLButtonElement;
    expect(btn.className).toContain('btn-outline');
    expect(btn.className).toContain('btn-md');
    fireEvent.click(btn);
    expect(onclick).toHaveBeenCalledTimes(1);
  });

  it('renders an anchor instead of a button when href is given', () => {
    render(ButtonHarness, { label: 'Explore', href: '/explore' });

    const anchor = screen.getByText('Explore').closest('a');
    expect(anchor).toHaveAttribute('href', '/explore');
    expect(screen.getByText('Explore').closest('button')).toBeNull();
  });

  it('disables the button and blocks clicks when disabled', () => {
    const onclick = vi.fn();
    render(ButtonHarness, { label: 'Nope', disabled: true, onclick });

    const btn = screen.getByText('Nope').closest('button') as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    fireEvent.click(btn);
    expect(onclick).not.toHaveBeenCalled();
  });
});
