import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDateTime: (iso: string) => `fmtt(${iso})`,
}));

const getAuditLog = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  admin: { getAuditLog: (...a: unknown[]) => getAuditLog(...a) },
}));

import AuditDetailModal from './AuditDetailModal.svelte';
import type { AuditLogEntry } from '$lib/api/client.svelte';

const listEntry = {
  id: 42,
  user_id: 10,
  username: 'alice',
  action: 'repo.create',
  resource_type: 'repo',
  resource_id: 5,
  resource_name: 'demo',
  ip_address: '127.0.0.1',
  created_at: '2026-09-01T10:00:00Z',
  details: null,
} as unknown as AuditLogEntry;

describe('AuditDetailModal.svelte', () => {
  beforeEach(() => {
    getAuditLog.mockReset();
  });

  it('fetches the full detail and renders resource, ip and details', async () => {
    getAuditLog.mockResolvedValue({ ...listEntry, details: 'created via web' });
    render(AuditDetailModal, { log: listEntry, onClose: () => {} });

    await waitFor(() =>
      expect(screen.getByText('created via web')).toBeInTheDocument()
    );
    expect(getAuditLog).toHaveBeenCalledWith(42);
    expect(screen.getByText('demo (#5)')).toBeInTheDocument();
    expect(screen.getByText('fmtt(2026-09-01T10:00:00Z)')).toBeInTheDocument();
    expect(screen.getByText('127.0.0.1')).toBeInTheDocument();
    expect(screen.getByText('alice (#10)')).toBeInTheDocument();
  });

  it('falls back to the no-details placeholder and shows errors inline', async () => {
    getAuditLog.mockResolvedValue({ ...listEntry, details: null });
    render(AuditDetailModal, { log: listEntry, onClose: () => {} });

    await waitFor(() =>
      expect(screen.getByText('admin.audit.no_details')).toBeInTheDocument()
    );
  });

  it('shows an error banner when the detail fetch fails', async () => {
    getAuditLog.mockRejectedValue(new Error('boom'));
    render(AuditDetailModal, { log: listEntry, onClose: () => {} });

    await waitFor(() => expect(screen.getByText('boom')).toBeInTheDocument());
  });

  it('closes via Escape and the cancel button', async () => {
    getAuditLog.mockResolvedValue(listEntry);
    const onClose = vi.fn();
    render(AuditDetailModal, { log: listEntry, onClose });

    await fireEvent.click(screen.getByText('common.cancel'));
    expect(onClose).toHaveBeenCalledTimes(1);

    await fireEvent.keyDown(document.querySelector('.modal-overlay') as HTMLElement, {
      key: 'Escape',
    });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
