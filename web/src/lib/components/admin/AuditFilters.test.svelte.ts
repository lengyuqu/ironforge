import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import AuditFilters from './AuditFilters.svelte';

describe('AuditFilters.svelte', () => {
  it('renders the predefined action groups and resource options', () => {
    render(AuditFilters, {
      actionFilter: '',
      resourceFilter: '',
      onApply: () => {},
      onClear: () => {},
    });

    const selects = document.querySelectorAll('select');
    expect(selects).toHaveLength(2);
    const [actionSelect, resourceSelect] = selects;
    // 15 predefined action groups.
    expect(actionSelect.querySelectorAll('option')).toHaveLength(15);
    expect(resourceSelect.querySelectorAll('option')).toHaveLength(4);

    expect(screen.getByText('user.login')).toBeInTheDocument();
    expect(screen.getByText('admin.delete_user')).toBeInTheDocument();
    // No filters set -> no clear button.
    expect(screen.queryByText('Clear filters')).toBeNull();
  });

  it('shows the clear button when a filter is active and invokes onClear', async () => {
    const onClear = vi.fn();
    render(AuditFilters, {
      actionFilter: 'repo.create',
      resourceFilter: '',
      onApply: () => {},
      onClear,
    });

    const clearBtn = screen.getByText('Clear filters');
    await fireEvent.click(clearBtn);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it('fires onApply when the action select changes', async () => {
    const onApply = vi.fn();
    render(AuditFilters, {
      actionFilter: '',
      resourceFilter: '',
      onApply,
      onClear: () => {},
    });

    const actionSelect = document.querySelectorAll('select')[0];
    await fireEvent.change(actionSelect, { target: { value: 'user.login' } });
    expect(onApply).toHaveBeenCalledTimes(1);
  });
});
