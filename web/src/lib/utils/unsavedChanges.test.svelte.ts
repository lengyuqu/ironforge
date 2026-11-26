import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import type { BeforeNavigate } from '@sveltejs/kit';
import type { UnsavedChangesGuard } from './unsavedChanges.svelte';

let navCallback: ((nav: BeforeNavigate) => void) | undefined;
const gotoMock = vi.fn();

vi.mock('$app/navigation', () => ({
  beforeNavigate: (cb: (nav: BeforeNavigate) => void) => {
    navCallback = cb;
  },
  goto: (...args: unknown[]) => gotoMock(...args),
}));

const confirmDialogMock = vi.fn();
vi.mock('$lib/components/confirm.svelte', () => ({
  confirmDialog: (...args: unknown[]) => confirmDialogMock(...args),
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string) => key,
}));

import UnsavedGuardHarness from '../../test-stubs/UnsavedGuardHarness.svelte';

function makeNavigation(opts: { url?: string; willUnload?: boolean; type?: 'link' | 'leave' } = {}): BeforeNavigate {
  const base = {
    from: null,
    to: { url: new URL(opts.url ?? 'http://localhost/x?y=1'), params: null, route: { id: null }, scroll: null },
    willUnload: opts.willUnload ?? false,
    delta: undefined,
    complete: Promise.resolve(),
    cancel: vi.fn(),
  };
  if (opts.type === 'leave') {
    return { ...base, type: 'leave' };
  }
  return { ...base, type: 'link', event: new PointerEvent('click') };
}

describe('useUnsavedChangesGuard', () => {
  beforeEach(() => {
    navCallback = undefined;
    gotoMock.mockReset();
    gotoMock.mockResolvedValue(undefined);
    confirmDialogMock.mockReset();
  });

  it('allows navigation without canceling when not dirty', () => {
    render(UnsavedGuardHarness, { dirty: false });
    const nav = makeNavigation();
    navCallback?.(nav);
    expect(nav.cancel).not.toHaveBeenCalled();
    expect(confirmDialogMock).not.toHaveBeenCalled();
  });

  it('cancels without opening ConfirmDialog when willUnload is true', () => {
    render(UnsavedGuardHarness, { dirty: true });
    const nav = makeNavigation({ type: 'leave', willUnload: true });
    navCallback?.(nav);
    expect(nav.cancel).toHaveBeenCalledTimes(1);
    expect(confirmDialogMock).not.toHaveBeenCalled();
  });

  it('prompts and navigates to the target path when the user confirms', async () => {
    confirmDialogMock.mockResolvedValue(true);
    render(UnsavedGuardHarness, { dirty: true });
    const nav = makeNavigation({ url: 'http://localhost/x?y=1' });
    navCallback?.(nav);

    expect(nav.cancel).toHaveBeenCalledTimes(1);
    expect(confirmDialogMock).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => expect(gotoMock).toHaveBeenCalledWith('/x?y=1'));
  });

  it('stays on the current page when the user declines', async () => {
    confirmDialogMock.mockResolvedValue(false);
    render(UnsavedGuardHarness, { dirty: true });
    const nav = makeNavigation({ url: 'http://localhost/x?y=1' });
    navCallback?.(nav);

    expect(nav.cancel).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => expect(confirmDialogMock).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    expect(gotoMock).not.toHaveBeenCalled();
  });

  it('does not intercept navigations triggered inside suspend()', async () => {
    let guard!: UnsavedChangesGuard;
    render(UnsavedGuardHarness, {
      dirty: true,
      onGuard: (g) => {
        guard = g;
      },
    });

    await guard.suspend(async () => {
      const nav = makeNavigation({ url: 'http://localhost/saved' });
      navCallback?.(nav);
      expect(nav.cancel).not.toHaveBeenCalled();
      expect(confirmDialogMock).not.toHaveBeenCalled();
    });

    // Guard becomes active again once suspend() resolves.
    const afterNav = makeNavigation({ url: 'http://localhost/elsewhere' });
    confirmDialogMock.mockResolvedValue(false);
    navCallback?.(afterNav);
    expect(afterNav.cancel).toHaveBeenCalledTimes(1);
  });
});
