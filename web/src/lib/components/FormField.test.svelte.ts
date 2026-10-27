import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

import FormFieldHarness from './FormFieldHarness.svelte';

describe('FormField.svelte (via FormFieldHarness)', () => {
  it('links the label to the control', () => {
    render(FormFieldHarness, { label: 'Repository name' });
    const input = screen.getByLabelText(/Repository name/);
    expect(input.tagName).toBe('INPUT');
  });

  it('uses a custom id when given', () => {
    render(FormFieldHarness, { label: 'Name', id: 'custom-id' });
    expect(screen.getByLabelText(/Name/)).toHaveAttribute('id', 'custom-id');
  });

  it('shows the hint, and replaces it with the error and flags the control invalid', async () => {
    const { rerender } = render(FormFieldHarness, { hint: 'Letters only' });
    const input = screen.getByLabelText(/Name/);
    expect(input).toHaveAccessibleDescription('Letters only');
    expect(input).not.toHaveAttribute('aria-invalid');

    await rerender({ hint: 'Letters only', error: 'Name is required' });
    expect(screen.queryByText('Letters only')).not.toBeInTheDocument();
    expect(input).toHaveAccessibleDescription('Name is required');
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByRole('alert')).toHaveTextContent('Name is required');
  });

  it('marks required and optional fields', () => {
    const { container, rerender } = render(FormFieldHarness, { required: true });
    expect(container.querySelector('.field-required')).toBeInTheDocument();
    rerender({ optional: true });
    expect(screen.getByText('(optional)')).toBeInTheDocument();
  });
});
