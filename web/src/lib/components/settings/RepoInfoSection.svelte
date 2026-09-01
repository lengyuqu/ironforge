<script lang="ts">
  import type { RepoInfo } from '$lib/types/entities';
  import { createT } from '$lib/i18n';

  const t = createT();

  let { repository }: { repository: RepoInfo } = $props();
</script>

<section class="section">
  <h2>{t('settings.repository_info.title', 'Repository Information')}</h2>
  <div class="info-grid">
    <div class="info-item">
      <span class="info-label">{t('settings.repository_info.name', 'Name')}</span>
      <div class="info-value">{repository.name}</div>
    </div>
    <div class="info-item">
      <span class="info-label">{t('settings.repository_info.description', 'Description')}</span>
      <div class="info-value">{repository.description || '-'}</div>
    </div>
    <div class="info-item">
      <span class="info-label">{t('settings.repository_info.visibility', 'Visibility')}</span>
      <div class="info-value">
        <span class="badge" class:private={repository.is_private}>
          {repository.is_private
            ? t('settings.repository_info.private', 'Private')
            : t('settings.repository_info.public', 'Public')}
        </span>
      </div>
    </div>
  </div>
</section>

<style>
  h2 {
    font-size: 1.25rem;
    margin-bottom: 1rem;
    color: var(--text-primary);
  }

  .info-grid {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
  }

  .info-item {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .info-label {
    font-size: 0.85rem;
    color: var(--text-secondary);
    font-weight: 500;
  }

  .info-value {
    color: var(--text-primary);
    font-size: 0.95rem;
  }

  .badge {
    display: inline-block;
    padding: 0.25rem 0.75rem;
    border-radius: 12px;
    font-size: 0.8rem;
    font-weight: 600;
    background: var(--green, #28a745);
    color: white;
  }

  .badge.private {
    background: var(--orange, #ff8800);
  }
</style>
