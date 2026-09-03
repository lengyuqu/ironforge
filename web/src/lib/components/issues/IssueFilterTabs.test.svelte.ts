import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import IssueFilterTabs from './IssueFilterTabs.svelte';

const milestones = [
  { id: 1, repo_id: 1, title: 'v1.0', state: 'open', open_issues: 2 },
  { id: 2, repo_id: 1, title: 'backlog', state: 'open', open_issues: 0 },
];

describe('IssueFilterTabs.svelte', () => {
  it('renders exactly the open/closed/all tabs', () => {
    render(IssueFilterTabs, { filter: 'open', onFilterChange: () => {} });

    expect(screen.getByText('issues.tabs.open')).toBeInTheDocument();
    expect(screen.getByText('issues.tabs.closed')).toBeInTheDocument();
    expect(screen.getByText('issues.tabs.all')).toBeInTheDocument();
    // IssueFilterTabs 没有 merged 维度（PR 专属），不应出现
    expect(screen.queryByText('issues.tabs.merged')).not.toBeInTheDocument();
  });

  it('marks only the active filter', () => {
    render(IssueFilterTabs, { filter: 'closed', onFilterChange: () => {} });

    const active = document.querySelector('.filter-btn.active');
    expect(active?.textContent?.trim()).toBe('issues.tabs.closed');
  });

  it('delegates filter changes to the parent', () => {
    const onFilterChange = vi.fn();
    render(IssueFilterTabs, { filter: 'open', onFilterChange });

    fireEvent.click(screen.getByText('issues.tabs.all'));
    expect(onFilterChange).toHaveBeenCalledWith('all');
  });

  it('does not render the milestone row without milestone props', () => {
    render(IssueFilterTabs, { filter: 'open', onFilterChange: () => {} });
    expect(screen.queryByText('issues.no_milestone')).not.toBeInTheDocument();
  });

  it('renders milestone chips when milestones are provided (M2-2 A1)', () => {
    render(IssueFilterTabs, {
      filter: 'open',
      onFilterChange: () => {},
      milestones,
      milestoneFilter: '',
      onMilestoneChange: () => {},
    });

    expect(screen.getByText('common.all')).toBeInTheDocument();
    expect(screen.getByText('issues.no_milestone')).toBeInTheDocument();
    expect(screen.getByText('v1.0')).toBeInTheDocument();
    expect(screen.getByText('backlog')).toBeInTheDocument();
    // Only milestones with open issues show the count bubble.
    expect(screen.getAllByText('2')).toHaveLength(1);
  });

  it('marks the active milestone chip and delegates selection', () => {
    const onMilestoneChange = vi.fn();
    render(IssueFilterTabs, {
      filter: 'open',
      onFilterChange: () => {},
      milestones,
      milestoneFilter: '1',
      onMilestoneChange,
    });

    // State tab ('open') and the milestone chip both carry .active.
    const actives = [...document.querySelectorAll('.filter-btn.active')];
    expect(actives.some((el) => el.textContent?.includes('v1.0'))).toBe(true);

    fireEvent.click(screen.getByText('issues.no_milestone'));
    expect(onMilestoneChange).toHaveBeenCalledWith('none');

    fireEvent.click(screen.getByText('backlog'));
    expect(onMilestoneChange).toHaveBeenCalledWith('2');
  });
});
