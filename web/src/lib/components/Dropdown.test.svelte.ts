import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

import DropdownHarness from './DropdownHarness.svelte';

describe('Dropdown.svelte (via DropdownHarness)', () => {
  it('toggles the menu from the trigger, reflecting aria-expanded', () => {
    render(DropdownHarness);

    const trigger = screen.getByText('Trigger').closest('button') as HTMLButtonElement;
    expect(trigger.getAttribute('aria-haspopup')).toBe('true');
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    expect(document.querySelector('.dropdown-menu')).not.toBeInTheDocument();

    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    expect(document.querySelector('.dropdown-menu')).toBeInTheDocument();
    expect(screen.getByText('Option A')).toBeInTheDocument();
  });

  it('closes the menu via a menu item wired to the close callback', () => {
    render(DropdownHarness);

    const trigger = screen.getByText('Trigger').closest('button') as HTMLButtonElement;
    fireEvent.click(trigger);
    fireEvent.click(screen.getByText('Option A'));

    expect(document.querySelector('.dropdown-menu')).not.toBeInTheDocument();
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
  });

  it('closes on Escape and outside clicks', () => {
    render(DropdownHarness);

    const trigger = screen.getByText('Trigger').closest('button') as HTMLButtonElement;
    fireEvent.click(trigger);

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(document.querySelector('.dropdown-menu')).not.toBeInTheDocument();

    fireEvent.click(trigger);
    // A click far outside both menu and trigger (on body) closes it.
    fireEvent.click(document.body);
    expect(document.querySelector('.dropdown-menu')).not.toBeInTheDocument();
  });
});
