import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// The real instance store is used: setBanner/clearBanner are its public API
// and the banner state is module-global, reset between cases below.

import InstanceBanner from './InstanceBanner.svelte';
import { setBanner, clearBanner } from '$lib/stores/instance.svelte';

describe('InstanceBanner.svelte', () => {
  it('renders nothing without a banner message', () => {
    clearBanner();
    const { container } = render(InstanceBanner);

    expect(container.querySelector('.banner')).not.toBeInTheDocument();
  });

  it('renders the message with the type icon and dismisses on close', () => {
    setBanner('Maintenance at 2am', 'warning');
    render(InstanceBanner);

    const banner = document.querySelector('.banner') as HTMLElement;
    expect(banner.classList.contains('warning')).toBe(true);
    expect(screen.getByText('Maintenance at 2am')).toBeInTheDocument();
    expect(banner.querySelector('.banner-icon')?.textContent).toContain('⚠️');

    fireEvent.click(screen.getByText('✕'));
    expect(document.querySelector('.banner')).not.toBeInTheDocument();
  });

  it('styles error banners with the error icon', () => {
    setBanner('Disk almost full', 'error');
    render(InstanceBanner);

    const banner = document.querySelector('.banner') as HTMLElement;
    expect(banner.classList.contains('error')).toBe(true);
    expect(banner.querySelector('.banner-icon')?.textContent).toContain('🚫');
  });
});
