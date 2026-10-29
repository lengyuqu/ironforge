import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

import SuccessBanner from './SuccessBanner.svelte';

describe('SuccessBanner.svelte', () => {
  it('announces the message politely as a status', () => {
    render(SuccessBanner, { message: 'Saved' });
    expect(screen.getByRole('status')).toHaveTextContent('Saved');
  });
});
