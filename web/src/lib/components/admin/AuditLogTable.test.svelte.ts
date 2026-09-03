import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
  formatDateTime: (iso: string) => `fmtt(${iso})`,
}));

import AuditLogTable from './AuditLogTable.svelte';
import type { AuditLogEntry } from '$lib/api/client.svelte';

function makeLog(overrides: Partial<AuditLogEntry>): AuditLogEntry {
  return {
    id: 1,
    user_id: 10,
    username: 'alice',
    action: 'repo.create',
    resource_type: 'repo',
    resource_id: 5,
    resource_name: 'demo',
    ip_address: '127.0.0.1',
    created_at: '2026-09-01T10:00:00Z',
    details: null,
    ...overrides,
  } as AuditLogEntry;
}

describe('AuditLogTable.svelte', () => {
  it('renders rows with user, action badge, resource and ip', () => {
    render(AuditLogTable, {
      logs: [makeLog({})],
      page: 0,
      totalPages: 1,
      onOpenDetail: () => {},
      onPrevPage: () => {},
      onNextPage: () => {},
    });

    expect(screen.getByText('alice')).toBeInTheDocument();
    const badge = document.querySelector('.action-badge');
    expect(badge?.getAttribute('data-action')).toBe('repo.create');
    expect(screen.getByText('Repository: demo')).toBeInTheDocument();
    expect(screen.getByText('127.0.0.1')).toBeInTheDocument();
    expect(screen.getByText('fmt(2026-09-01T10:00:00Z)')).toBeInTheDocument();
    // Single page -> no pagination controls.
    expect(screen.queryByText('← Prev')).toBeNull();
  });

  it('falls back to #user_id and em-dashes for missing fields', () => {
    render(AuditLogTable, {
      logs: [
        makeLog({ id: 2, username: null, user_id: 7, resource_name: null, ip_address: null }),
        makeLog({ id: 3, username: null, user_id: null, resource_name: null, ip_address: null }),
      ],
      page: 0,
      totalPages: 1,
      onOpenDetail: () => {},
      onPrevPage: () => {},
      onNextPage: () => {},
    });

    expect(screen.getByText('#7')).toBeInTheDocument();
    // Two anonymous rows with em-dash user cells.
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(2);
  });

  it('invokes onOpenDetail with the row entry', async () => {
    const onOpenDetail = vi.fn();
    const log = makeLog({});
    render(AuditLogTable, {
      logs: [log],
      page: 0,
      totalPages: 1,
      onOpenDetail,
      onPrevPage: () => {},
      onNextPage: () => {},
    });

    await fireEvent.click(screen.getByText('admin.audit.fields.details'));
    expect(onOpenDetail).toHaveBeenCalledWith(log);
  });

  it('paginates: prev disabled on the first page, next advances', async () => {
    const onPrevPage = vi.fn();
    const onNextPage = vi.fn();
    render(AuditLogTable, {
      logs: [makeLog({})],
      page: 0,
      totalPages: 3,
      onOpenDetail: () => {},
      onPrevPage,
      onNextPage,
    });

    expect(screen.getByText('← Prev')).toBeDisabled();
    await fireEvent.click(screen.getByText('Next →'));
    expect(onNextPage).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Page 1 of 3')).toBeInTheDocument();
  });
});
