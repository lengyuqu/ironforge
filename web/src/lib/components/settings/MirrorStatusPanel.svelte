<script lang="ts">
  import type { RepositoryMirror } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let { mirror }: { mirror: RepositoryMirror } = $props();

  function formatDate(value: string | null) {
    return value ? new Date(value).toLocaleString() : t('common.never');
  }
</script>

<section class="section">
  <h2>{t('settings.mirror.status')}</h2>
  <dl class="status-grid">
    <div>
      <dt>{t('settings.mirror.state')}</dt>
      <dd><span class:error-state={mirror.status === 'error'}>{mirror.status}</span></dd>
    </div>
    <div>
      <dt>{t('settings.mirror.last_sync')}</dt>
      <dd>{formatDate(mirror.last_sync_at)}</dd>
    </div>
    <div>
      <dt>{t('settings.mirror.next_sync')}</dt>
      <dd>{formatDate(mirror.next_sync_at)}</dd>
    </div>
  </dl>

  {#if mirror.last_sync_error}
    <div class="error-detail">{mirror.last_sync_error}</div>
  {/if}
</section>

<style>
  h2 {
    font-size: 1.1rem;
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  .section {
    margin-bottom: 2.5rem;
    padding-bottom: 2rem;
    border-bottom: 1px solid var(--border);
  }

  .status-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 1rem;
    margin: 0;
  }

  .status-grid div {
    padding: 1rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-secondary);
  }

  dt {
    margin-bottom: 0.35rem;
    color: var(--text-secondary);
    font-size: 0.8rem;
    font-weight: 600;
    text-transform: uppercase;
  }

  dd {
    margin: 0;
    color: var(--text-primary);
  }

  .error-state {
    color: var(--red);
    font-weight: 600;
  }

  .error-detail {
    padding: 1rem;
    border-radius: 6px;
    margin-bottom: 1rem;
    background: rgba(248, 81, 73, 0.12);
    color: var(--red);
  }

  @media (max-width: 720px) {
    .status-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
