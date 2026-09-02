import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import LabelGrid from './LabelGrid.svelte';
import type { Label } from '$lib/types/entities';

const label: Label = {
  id: 3,
  name: 'bug',
  color: '#ff0000',
  description: 'Something is broken',
};

describe('LabelGrid.svelte', () => {
  it('renders name, colour swatch and description', () => {
    render(LabelGrid, { items: [label], onEdit: () => {}, onDelete: () => {} });

    expect(screen.getByText('bug')).toBeInTheDocument();
    expect(screen.getByText('Something is broken')).toBeInTheDocument();
    const swatch = document.querySelector('.label-color');
    expect(swatch).not.toBeNull();
    expect(swatch!.getAttribute('style')).toContain('#ff0000');
  });

  it('omits the description node when description is null', () => {
    render(LabelGrid, {
      items: [{ ...label, description: null }],
      onEdit: () => {},
      onDelete: () => {},
    });
    expect(screen.queryByText('Something is broken')).not.toBeInTheDocument();
  });

  it('delegates edit and delete intents with the label payload', () => {
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    render(LabelGrid, { items: [label], onEdit, onDelete });

    fireEvent.click(screen.getByTitle('Edit'));
    expect(onEdit).toHaveBeenCalledWith(label);

    fireEvent.click(screen.getByTitle('Delete'));
    expect(onDelete).toHaveBeenCalledWith(label);
  });
});
