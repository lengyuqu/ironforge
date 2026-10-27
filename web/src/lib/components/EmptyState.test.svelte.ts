import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

import EmptyState from './EmptyState.svelte';
import LoadingState from './LoadingState.svelte';

describe('EmptyState.svelte', () => {
  it('renders the title, and the description only when given', () => {
    const { rerender } = render(EmptyState, { title: 'No issues yet' });
    expect(screen.getByText('No issues yet')).toBeInTheDocument();
    expect(document.querySelector('.empty-description')).toBeNull();

    rerender({ title: 'No issues yet', description: 'Create one', icon: '📭' });
    expect(screen.getByText('Create one')).toBeInTheDocument();
    expect(document.querySelector('.empty-icon')).toHaveAttribute('aria-hidden', 'true');
  });
});

describe('LoadingState.svelte', () => {
  it('announces a single status region with the default label', () => {
    render(LoadingState);
    expect(screen.getAllByRole('status')).toHaveLength(1);
    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('accepts a custom label', () => {
    render(LoadingState, { label: 'Fetching runners' });
    expect(screen.getByText('Fetching runners')).toBeInTheDocument();
  });
});
