import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import RepoToolbar from './RepoToolbar.svelte';
import type { Branch } from '$lib/types/entities';

const branches: Branch[] = [
  { name: 'main', is_default: true },
  { name: 'feature', is_default: false },
];

const baseProps = {
  owner: 'alice',
  repo: 'demo',
  ref: 'main',
  path: '',
  branches,
  currentRefLabel: 'main',
  onSelectBranch: () => {},
};

describe('RepoToolbar.svelte', () => {
  beforeEach(() => {});

  it('renders the branch trigger, new-file link and repo breadcrumb', () => {
    render(RepoToolbar, { ...baseProps });

    expect(screen.getByLabelText('repo.select_branch')).toBeInTheDocument();
    const newFile = screen.getByText((c) => c.includes('repo.new_file')).closest('a') as HTMLAnchorElement;
    expect(newFile.getAttribute('href')).toContain('/alice/demo/new');
    const crumb = screen.getByText('demo').closest('a') as HTMLAnchorElement;
    expect(crumb.getAttribute('href')).toContain('/alice/demo');
  });

  it('selects a branch through the dropdown and reports it via onSelectBranch', async () => {
    const onSelectBranch = vi.fn();
    render(RepoToolbar, { ...baseProps, onSelectBranch });

    await fireEvent.click(screen.getByLabelText('repo.select_branch'));
    const item = screen.getByText('feature') as HTMLButtonElement;
    expect(item.className).toContain('dropdown-item');
    await fireEvent.click(item);

    await waitFor(() => expect(onSelectBranch).toHaveBeenCalledWith('feature'));
  });

  it('marks the current ref as active in the branch dropdown', async () => {
    render(RepoToolbar, { ...baseProps });

    await fireEvent.click(screen.getByLabelText('repo.select_branch'));
    const active = document.querySelector('.dropdown-item.active');
    expect(active?.textContent).toContain('main');
    expect(active?.textContent).toContain('repo.browser.default_branch');
  });

  it('splits a nested path into breadcrumb parts', () => {
    render(RepoToolbar, { ...baseProps, path: 'src/lib' });

    expect(screen.getByText('src')).toBeInTheDocument();
    expect(screen.getByText('lib')).toBeInTheDocument();
  });
});
