<script lang="ts">
  // Landing hero — logo, tagline and sign-up / sign-in actions.
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import BrandMark from '$lib/components/BrandMark.svelte';

  const t = createT();

  function handleRegister() {
    goto('/register');
  }

  function handleLogin() {
    goto('/login');
  }
</script>

<section class="hero">
  <div class="hero-content">
    <div class="hero-logo">
      <BrandMark size={64} />
    </div>
    <h1 class="hero-title">IronForge</h1>
    <p class="hero-subtitle">{t('home.tagline')}</p>
    <p class="hero-desc">{t('home.description')}</p>

    <div class="hero-actions">
      <button class="btn-hero-primary" onclick={handleRegister}>
        {t('home.sign_up')}
      </button>
      <button class="btn-hero-secondary" onclick={handleLogin}>
        {t('home.sign_in')}
      </button>
    </div>
  </div>
</section>

<style>
  .hero {
    background:
      radial-gradient(65% 80% at 50% 0%, color-mix(in srgb, var(--brand-ember) 14%, transparent), transparent 70%),
      radial-gradient(50% 60% at 85% 20%, color-mix(in srgb, var(--accent) 12%, transparent), transparent 65%),
      linear-gradient(180deg, var(--bg-secondary) 0%, var(--bg-primary) 100%);
    color: var(--text-primary);
    border-bottom: 1px solid var(--border);
    padding: 80px 24px;
    text-align: center;
  }

  .hero-content {
    max-width: 800px;
    margin: 0 auto;
  }

  .hero-logo {
    margin-bottom: 24px;
    display: flex;
    justify-content: center;
  }

  .hero-title {
    font-size: 48px;
    font-weight: 700;
    margin-bottom: 16px;
    letter-spacing: -1px;
  }

  .hero-subtitle {
    font-size: 20px;
    font-weight: 500;
    margin-bottom: 12px;
    color: var(--text-primary);
  }

  .hero-desc {
    font-size: 16px;
    color: var(--text-secondary);
    margin-bottom: 32px;
    max-width: 640px;
    margin-left: auto;
    margin-right: auto;
    line-height: 1.6;
  }

  .hero-actions {
    display: flex;
    gap: 16px;
    justify-content: center;
    flex-wrap: wrap;
  }

  .btn-hero-primary {
    padding: 12px 32px;
    background: var(--green-dim);
    color: var(--text-on-accent);
    border: 1px solid var(--green-dim);
    border-radius: var(--radius-lg);
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s, border-color 0.2s;
  }

  .btn-hero-primary:hover {
    background: var(--green);
    border-color: var(--green);
  }

  .btn-hero-secondary {
    padding: 12px 32px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s, border-color 0.2s;
  }

  .btn-hero-secondary:hover {
    background: var(--bg-hover);
    border-color: var(--accent);
  }
</style>
