import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

import LayoutHarness from './LayoutHarness.svelte';

describe('Layout.svelte (via LayoutHarness)', () => {
  it('wraps the page body in the layout shell', () => {
    const { container } = render(LayoutHarness);

    expect(container.querySelector('.page-layout')).toBeInTheDocument();
    expect(container.querySelector('.page-layout__inner')).toBeInTheDocument();
    // Children render inside the inner shell.
    expect(screen.getByText('Page body').closest('.page-layout__inner')).not.toBeNull();
  });
});
