import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

import ErrorBanner from './ErrorBanner.svelte';

describe('ErrorBanner.svelte', () => {
  it('announces the message as an alert', () => {
    render(ErrorBanner, { message: 'Could not load' });
    expect(screen.getByRole('alert')).toHaveTextContent('Could not load');
  });
});
