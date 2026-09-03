import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

vi.mock('$app/environment', () => ({ browser: true, dev: false }));

const writeText = vi.fn().mockResolvedValue(undefined);
beforeEach(() => {
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    configurable: true,
    writable: true,
  });
  writeText.mockClear();
});

import EmptyRepoGuide from './EmptyRepoGuide.svelte';

const baseProps = { owner: 'alice', repo: 'demo', defaultBranch: 'main' };

describe('EmptyRepoGuide.svelte', () => {
  it('renders the setup title, steps and quick-command block for the default branch', () => {
    render(EmptyRepoGuide, { ...baseProps });

    expect(screen.getByText('repo.empty.title')).toBeInTheDocument();
    expect(screen.getByText('repo.empty.step_clone')).toBeInTheDocument();
    const quick = document.querySelector('.quick-commands code');
    expect(quick?.textContent).toContain('git branch -M main');
    expect(quick?.textContent).toContain('/git/alice/demo');
    expect(screen.getByText('HTTPS')).toBeInTheDocument();
    expect(screen.getByText('SSH')).toBeInTheDocument();
  });

  it('copies the HTTP clone URL when the HTTPS copy button is clicked', async () => {
    render(EmptyRepoGuide, { ...baseProps });

    const buttons = screen.getAllByText((c) => c.includes('repo.empty.copy'));
    await fireEvent.click(buttons[0]);
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('/git/alice/demo'));
    expect(screen.getAllByText((c) => c.includes('repo.empty.copied')).length).toBeGreaterThan(0);
  });

  it('copies an SSH clone URL (built from the browser host) when SSH copy is clicked', async () => {
    render(EmptyRepoGuide, { ...baseProps });

    const buttons = screen.getAllByText((c) => c.includes('repo.empty.copy'));
    await fireEvent.click(buttons[1]);
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining('ssh://git@'))
    );
  });

  it('uses the provided default branch in the quick commands', () => {
    render(EmptyRepoGuide, { owner: 'alice', repo: 'demo', defaultBranch: 'develop' });
    const quick = document.querySelector('.quick-commands code');
    expect(quick?.textContent).toContain('git branch -M develop');
  });
});
