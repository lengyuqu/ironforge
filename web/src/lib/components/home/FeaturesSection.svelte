<script lang="ts">
  // Landing features grid — six static feature cards.
  import { createT } from '$lib/i18n';

  const t = createT();

  const features = [
    { icon: '🚀', key: 'lightweight' },
    { icon: '🔧', key: 'all_in_one' },
    { icon: '🔒', key: 'security' },
    { icon: '🌐', key: 'self_hosted' },
    { icon: '⚡', key: 'fast' },
    { icon: '🛠️', key: 'extensible' },
  ] as const;
</script>

<section class="features">
  <div class="features-container">
    <h2 class="features-title">{t('home.features.title')}</h2>
    <div class="features-grid">
      {#each features as feature (feature.key)}
        <div class="feature-card">
          <div class="feature-icon">{feature.icon}</div>
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
    background: var(--bg-primary, #ffffff);
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
    color: var(--text-primary, #1f2328);
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
    border: 1px solid var(--border, #d0d7de);
    border-radius: 12px;
    transition: border-color 0.2s, box-shadow 0.2s;
  }

  .feature-card:hover {
    border-color: var(--primary, #2da44e);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
  }

  .feature-icon {
    font-size: 40px;
    margin-bottom: 16px;
  }

  .feature-card h3 {
    font-size: 18px;
    font-weight: 600;
    margin-bottom: 8px;
    color: var(--text-primary, #1f2328);
  }

  .feature-card p {
    font-size: 14px;
    color: var(--text-secondary, #656d76);
    line-height: 1.6;
  }
</style>
