<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import BrandMark from '$lib/components/BrandMark.svelte';
  import { auth } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let email = $state('');
  let loading = $state(false);
  let success = $state('');
  let localError = $state('');

  async function handleSubmit(e: Event) {
    e.preventDefault();
    localError = '';
    success = '';

    const trimmedEmail = email.trim();
    if (!trimmedEmail) {
      localError = t('auth.forgot.email_required');
      return;
    }

    loading = true;
    try {
      const res = await auth.forgotPassword(trimmedEmail);
      success = res.message || t('auth.forgot.sent');
    } catch (e) {
      localError = toErrorMessage(e, t('errors.reset_failed'));
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>{t('auth.forgot.title')} · IronForge</title>
</svelte:head>

<div class="login-page">
  <div class="login-card surface">
    <div class="login-header">
      <BrandMark size={40} />
      <h1>{t('auth.forgot.title')}</h1>
      <p>{t('auth.forgot.desc')}</p>
    </div>

    {#if localError}
      <ErrorBanner message={localError} />
    {/if}

    {#if success}
      <SuccessBanner message={success} />
    {/if}

    <form onsubmit={handleSubmit}>
      <label>
        {t('auth.register.email')}
        <input type="email" bind:value={email} required autocomplete="email" disabled={loading} />
      </label>

      <button type="submit" class="btn-primary" disabled={loading || !email.trim()}>
        {loading ? t('auth.forgot.sending') : t('auth.forgot.submit')}
      </button>
    </form>

    <p class="footer">
      <a href="/login">{t('auth.back_to_login')}</a>
    </p>
  </div>
</div>

<style>
  .login-card {
    width: 100%;
    max-width: 360px;
    padding: 32px;
  }

  .login-header {
    text-align: center;
    margin-bottom: 24px;
  }

  h1 {
    margin: 12px 0 6px;
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.5;
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
  }

  .footer a {
    color: var(--accent);
    text-decoration: none;
  }
</style>
