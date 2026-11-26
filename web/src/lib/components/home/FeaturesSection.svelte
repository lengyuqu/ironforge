<script lang="ts">
  // Landing features grid — six static feature cards.
  import { createT } from '$lib/i18n';
  import Icon, { type IconName } from '$lib/components/Icon.svelte';

  const t = createT();

  const features: ReadonlyArray<{ icon: IconName; key: string }> = [
    { icon: 'zap', key: 'lightweight' },
    { icon: 'layers', key: 'all_in_one' },
    { icon: 'shield', key: 'security' },
    { icon: 'server', key: 'self_hosted' },
    { icon: 'cpu', key: 'fast' },
    { icon: 'agents', key: 'extensible' },
  ];
</script>

<section class="features">
  <div class="features-container">
    <h2 class="features-title">{t('home.features.title')}</h2>
    <div class="features-grid">
      {#each features as feature (feature.key)}
        <div class="feature-card">
          <div class="feature-icon"><Icon name={feature.icon} size={28} /></div>
          <h3>{t(`home.features.${feature.key}.title`)}</h3>
          <p>{t(`home.features.${feature.key}.desc`)}</p>
        </div>
      {/each}
    </div>
  </div>
</section>

<style>
  .features {
    padding: 80px 24px;
    background: var(--bg-primary);
  }

  .features-container {
    max-width: 1200px;
    margin: 0 auto;
  }

  .features-title {
    text-align: center;
    font-size: 32px;
    font-weight: 700;
    margin-bottom: 48px;
    color: var(--text-primary);
  }

  .features-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 32px;
  }

  @media (max-width: 900px) {
    .features-grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }

  @media (max-width: 600px) {
    .features-grid {
      grid-template-columns: 1fr;
    }
  }

  .feature-card {
    padding: 24px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--bg-secondary);
    transition: border-color 0.2s, box-shadow 0.2s;
  }

  .feature-card:hover {
    border-color: var(--accent);
    box-shadow: var(--shadow-md);
  }

  .feature-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 48px;
    height: 48px;
    border-radius: var(--radius-lg);
    background: var(--accent-subtle);
    color: var(--accent);
    margin-bottom: 16px;
  }

  .feature-card h3 {
    font-size: 18px;
    font-weight: 600;
    margin-bottom: 8px;
    color: var(--text-primary);
  }

  .feature-card p {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
  }
</style>
