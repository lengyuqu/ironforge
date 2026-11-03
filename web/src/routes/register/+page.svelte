<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import BrandMark from '$lib/components/BrandMark.svelte';
  import { register, getAuthError, getAuthLoading } from '$lib/stores/auth.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let username = $state('');
  let email = $state('');
  let password = $state('');
  let localError = $state('');

  async function handleSubmit(e: Event) {
    e.preventDefault();
    localError = '';
    const ok = await register(username, email, password);
    if (ok) {
      window.location.href = '/dashboard';
    } else {
      localError = getAuthError() || t('auth.register.failed');
    }
  }
</script>

<svelte:head>
  <title>{t('auth.register.title')} · IronForge</title>
</svelte:head>

<div class="login-page">
  <div class="login-card surface">
    <div class="login-header">
      <BrandMark size={40} />
      <h1>{t('auth.register.title')}</h1>
    </div>

    {#if localError}
      <ErrorBanner message={localError} />
    {/if}

    <form onsubmit={handleSubmit}>
      <label>
        {t('auth.register.username')}
        <input type="text" bind:value={username} required autocomplete="username" />
      </label>

      <label>
        {t('auth.register.email')}
        <input type="email" bind:value={email} required autocomplete="email" />
      </label>

      <label>
        {t('auth.register.password')}
        <input type="password" bind:value={password} required autocomplete="new-password" minlength={6} />
      </label>

      <button type="submit" class="btn-primary" disabled={getAuthLoading()}>
        {getAuthLoading() ? t('auth.register.submitting') : t('auth.register.submit')}
      </button>
    </form>

    <p class="footer">
      {t('auth.register.footer', { link: '' })}
      <a href="/login">{t('auth.register.footer_link')}</a>
    </p>
  </div>
</div>

<style>

  .login-card {
    width: 100%;
    max-width: 340px;
    padding: 32px;
  }

  .login-header {
    text-align: center;
    margin-bottom: 24px;
  }

  h1 {
    margin-top: 12px;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }
.footer {
    text-align: center;
    margin-top: 20px;
    font-size: 13px;
    color: var(--text-secondary);
  }
</style>
