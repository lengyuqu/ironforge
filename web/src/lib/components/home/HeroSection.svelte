<script lang="ts">
  // Landing hero — logo, tagline and sign-up / sign-in actions.
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';

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
      <svg viewBox="0 0 16 16" width="64" height="64" fill="currentColor" aria-hidden="true">
        <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
      </svg>
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
    background: var(--primary, #2da44e);
    color: white;
    border: none;
    border-radius: 8px;
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  .btn-hero-primary:hover {
    background: var(--primary-hover, #218838);
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
