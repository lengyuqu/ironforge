import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';

describe('LoadingSpinner.svelte', () => {
  it('renders with default size=20 and accessibility attributes', () => {
    render(LoadingSpinner);
    const span = screen.getByRole('status');
    expect(span).toBeInTheDocument();
    expect(span).toHaveAttribute('aria-label', 'Loading');
    const svg = span.querySelector('svg');
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute('width', '20');
    expect(svg).toHaveAttribute('height', '20');
  });

  it('respects custom size and className props', () => {
    render(LoadingSpinner, { size: 48, className: 'block' });
    const svg = document.querySelector('svg');
    expect(svg).toHaveAttribute('width', '48');
    expect(svg).toHaveAttribute('height', '48');
    const span = screen.getByRole('status');
    expect(span.classList.contains('block')).toBe(true);
  });
});
