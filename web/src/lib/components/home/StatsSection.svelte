<script lang="ts">
  // Landing stats band — four static highlight numbers.
  import { createT } from '$lib/i18n';

  const t = createT();

  const stats = [
    { number: '50MB', key: 'memory' },
    { number: '1', key: 'binary' },
    { number: '100%', key: 'compatible' },
    { number: 'Rust', key: 'language' },
  ] as const;
</script>

<section class="stats">
  <div class="stats-container">
    {#each stats as stat (stat.key)}
      <div class="stat-item">
        <div class="stat-number">{stat.number}</div>
        <div class="stat-label">{t(`home.stats.${stat.key}`)}</div>
      </div>
    {/each}
  </div>
</section>

<style>
  .stats {
    padding: 60px 24px;
    background: var(--bg-secondary);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .stats-container {
    max-width: 1200px;
    margin: 0 auto;
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 32px;
    text-align: center;
  }

  @media (max-width: 720px) {
    .stats-container {
      grid-template-columns: repeat(2, 1fr);
    }
  }

  .stat-item {
    padding: 16px;
  }

  .stat-number {
    font-size: 36px;
    font-weight: 700;
    color: var(--green);
    margin-bottom: 8px;
  }

  .stat-label {
    font-size: 14px;
    color: var(--text-secondary);
  }
</style>
