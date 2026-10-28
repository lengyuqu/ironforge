<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import Modal from '$lib/components/Modal.svelte';
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
      .catch((e) => {
        detailError = toErrorMessage(e, t('errors.load_failed'));
      })
      .finally(() => {
        detailLoading = false;
      });
  });


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

<Modal open={true} title={t('admin.audit.detail_title', { id: detail.id })} {onClose}>
  {#if detailError}
    <ErrorBanner message={detailError} />
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
      <span class="detail-value"><span class="action-badge surface" data-action={detail.action}>{detail.action}</span></span>
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
</Modal>

<style>
  .detail-grid { margin-bottom: 0; }

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
    font-size: 0.78rem;
    font-weight: 500;
    white-space: nowrap;
    font-family: var(--font-mono);
  }
  .action-badge[data-action^="user."] { border-color: var(--accent); color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .action-badge[data-action^="repo."] { border-color: var(--green); color: var(--green); background: color-mix(in srgb, var(--green) 10%, transparent); }
  .action-badge[data-action^="org."] { border-color: var(--purple); color: var(--purple); background: color-mix(in srgb, var(--purple) 10%, transparent); }
  .action-badge[data-action^="admin."] { border-color: var(--red); color: var(--red); background: color-mix(in srgb, var(--red) 10%, transparent); }

</style>
