<script lang="ts">
  import { createT, formatDateTime } from '$lib/i18n';
  import { admin, type AuditLogEntry } from '$lib/api/client.svelte';

  const t = createT();

  let {
    log,
    onClose,
  }: {
    log: AuditLogEntry;
    onClose: () => void;
  } = $props();

  let detail = $state<AuditLogEntry>(log);
  let detailLoading = $state(true);
  let detailError = $state('');

  // Fetch full detail on mount; fall back to the list entry on failure.
  $effect(() => {
    const id = log.id;
    detailLoading = true;
    detailError = '';
    admin
      .getAuditLog(id)
      .then((full) => {
        detail = full;
      })
      .catch((e: any) => {
        detailError = e?.message || t('errors.load_failed');
      })
      .finally(() => {
        detailLoading = false;
      });
  });

  function closeByKey(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onClose();
    }
  }

  function formatResourceType(rt: string | null): string {
    if (!rt) return '—';
    const map: Record<string, string> = {
      user: 'User',
      repo: 'Repository',
      org: 'Organization',
    };
    return map[rt] || rt;
  }
</script>

<div
  class="modal-overlay"
  onclick={onClose}
  role="button"
  tabindex="0"
  onkeydown={closeByKey}
>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1">
    <h2>{t('admin.audit.detail_title', { id: detail.id })}</h2>

    {#if detailError}
      <div class="detail-error">{detailError}</div>
    {/if}

    <div class="detail-grid">
      <div class="detail-row">
        <span class="detail-label">{t('admin.audit.fields.time')}</span>
        <span class="detail-value">{formatDateTime(detail.created_at)}</span>
      </div>
      <div class="detail-row">
        <span class="detail-label">{t('admin.audit.fields.user')}</span>
        <span class="detail-value">
          {#if detail.username}
            {detail.username} (#{detail.user_id})
          {:else}
            {detail.user_id ? `#${detail.user_id}` : '—'}
          {/if}
        </span>
      </div>
      <div class="detail-row">
        <span class="detail-label">{t('admin.audit.fields.action')}</span>
        <span class="detail-value"><span class="action-badge" data-action={detail.action}>{detail.action}</span></span>
      </div>
      <div class="detail-row">
        <span class="detail-label">{t('admin.audit.fields.resource_type')}</span>
        <span class="detail-value">{formatResourceType(detail.resource_type)}</span>
      </div>
      {#if detail.resource_name}
        <div class="detail-row">
          <span class="detail-label">{t('admin.audit.fields.resource')}</span>
          <span class="detail-value">{detail.resource_name} (#{detail.resource_id})</span>
        </div>
      {/if}
      <div class="detail-row">
        <span class="detail-label">{t('admin.audit.fields.ip')}</span>
        <span class="detail-value">{detail.ip_address || '—'}</span>
      </div>
      <div class="detail-row detail-row-full">
        <span class="detail-label">{t('admin.audit.fields.details')}</span>
        <span class="detail-value">
          {detailLoading ? t('common.loading') : detail.details || t('admin.audit.no_details')}
        </span>
      </div>
    </div>

    <div class="modal-actions">
      <button class="btn-secondary" onclick={onClose}>{t('common.cancel')}</button>
    </div>
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .modal {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 1.5rem;
    width: 560px;
    max-width: 90vw;
    max-height: 80vh;
    overflow-y: auto;
  }
  .modal h2 { margin: 0 0 1rem; font-size: 1.1rem; }

  .detail-error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    margin-bottom: 1rem;
  }

  .detail-grid { margin-bottom: 1rem; }
  .detail-row {
    display: flex;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
    gap: 1rem;
  }
  .detail-row:last-child { border-bottom: none; }
  .detail-label {
    min-width: 100px;
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-secondary);
    flex-shrink: 0;
  }
  .detail-value {
    font-size: 0.9rem;
    color: var(--text-primary);
    word-break: break-all;
  }
  .detail-row-full {
    flex-direction: column;
    gap: 0.25rem;
  }

  .action-badge {
    display: inline-block;
    padding: 0.15rem 0.5rem;
    border-radius: 8px;
    font-size: 0.78rem;
    font-weight: 500;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    white-space: nowrap;
    font-family: var(--font-mono, monospace);
  }
  .action-badge[data-action^="user."] { border-color: #58a6ff; color: #58a6ff; background: rgba(88, 166, 255, 0.1); }
  .action-badge[data-action^="repo."] { border-color: #3fb950; color: #3fb950; background: rgba(63, 185, 80, 0.1); }
  .action-badge[data-action^="org."] { border-color: #d2a8ff; color: #d2a8ff; background: rgba(210, 168, 255, 0.1); }
  .action-badge[data-action^="admin."] { border-color: #f85149; color: #f85149; background: rgba(248, 81, 73, 0.1); }

  .modal-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
    margin-top: 1rem;
  }
  .btn-secondary {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    border-radius: 6px;
    padding: 0.5rem 1rem;
    cursor: pointer;
    font-size: 0.9rem;
  }
</style>
