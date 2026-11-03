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
    background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);
    color: white;
    padding: 80px 24px;
    text-align: center;
  }

  .hero-content {
    max-width: 800px;
    margin: 0 auto;
  }

  .hero-logo {
    margin-bottom: 24px;
    opacity: 0.9;
  }

  .hero-title {
    font-size: 48px;
    font-weight: 700;
    margin-bottom: 16px;
    letter-spacing: -1px;
  }

  .hero-subtitle {
    font-size: 20px;
    font-weight: 400;
    margin-bottom: 12px;
    opacity: 0.9;
  }

  .hero-desc {
    font-size: 16px;
    opacity: 0.7;
    margin-bottom: 32px;
    max-width: 600px;
    margin-left: auto;
    margin-right: auto;
    line-height: 1.6;
  }

  .hero-actions {
    display: flex;
    gap: 16px;
    justify-content: center;
  }

  .btn-hero-primary {
    padding: 12px 32px;
    background: var(--green-dim);
    color: white;
    border: none;
    border-radius: 8px;
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  .btn-hero-primary:hover {
    background: var(--green);
  }

  .btn-hero-secondary {
    padding: 12px 32px;
    background: rgba(255, 255, 255, 0.1);
    color: white;
    border: 1px solid rgba(255, 255, 255, 0.3);
    border-radius: 8px;
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  .btn-hero-secondary:hover {
    background: rgba(255, 255, 255, 0.2);
  }
</style>
