<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import BrandMark from '$lib/components/BrandMark.svelte';
  import {
    login,
    verifyMfa,
    getAuthError,
    getAuthLoading,
    isLoggedIn,
    isMfaRequired,
    beginMfa,
  } from '$lib/stores/auth.svelte';
  import { createT } from '$lib/i18n';
  import { auth, type PublicSsoProvider } from '$lib/api/client.svelte';
  import { goto } from '$app/navigation';

  const t = createT();

  let username = $state('');
  let password = $state('');
  let mfaCode = $state('');
  let useBackupCode = $state(false);
  let localError = $state('');
  let ssoProviders = $state<PublicSsoProvider[]>([]);
  let ssoLoading = $state(true);

  // Redirect if already logged in (prevents flash of login form for authenticated users)
  $effect(() => {
    if (isLoggedIn()) {
      goto('/dashboard');
    }
  });

  $effect(() => {
    const params = new URLSearchParams(window.location.search);
    if (params.get('sso_mfa_required') === '1') {
      const usernameParam = params.get('username');
      if (usernameParam) {
        beginMfa(usernameParam);
        window.history.replaceState({}, '', '/login');
      }
    }
    loadSsoProviders();
  });

  async function loadSsoProviders() {
    try {
      ssoLoading = true;
      ssoProviders = (await auth.listSsoProviders()).filter(
        (provider) => provider.provider_type !== 'ldap',
      );
    } catch {
      ssoProviders = [];
    } finally {
      ssoLoading = false;
    }
  }

  // Same-origin path to return to after login (`?redirect=`). Hard-fails to
  // /dashboard unless it is a relative path starting with '/' (blocks
  // `//evil.com` and absolute-URL open redirects).
  function postLoginTarget(): string {
    const params = new URLSearchParams(window.location.search);
    const target = params.get('redirect');
    if (target && target.startsWith('/') && !target.startsWith('//')) {
      return target;
    }
    return '/dashboard';
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    localError = '';
    const ok = await login(username, password);
    if (ok) {
      window.location.href = postLoginTarget();
    } else if (isMfaRequired()) {
      localError = '';
    } else {
      localError = getAuthError() || t('auth.login.failed');
    }
  }

  async function handleMfaSubmit(e: Event) {
    e.preventDefault();
    localError = '';
    const ok = await verifyMfa(mfaCode.trim(), useBackupCode);
    if (ok) {
      window.location.href = postLoginTarget();
    } else {
      localError = getAuthError() || t('auth.login.mfa_failed');
    }
  }
</script>

<svelte:head>
  <title>{t('auth.login.title')} · IronForge</title>
</svelte:head>

<div class="login-page">
  <div class="login-card surface">
    <div class="login-header">
      <BrandMark size={40} />
      <h1>{t('auth.login.title')}</h1>
    </div>

    {#if localError}
      <ErrorBanner message={localError} />
    {/if}

    {#if isMfaRequired()}
      <form onsubmit={handleMfaSubmit}>
        <label>
          {t('auth.login.mfa_code')}
          <input
            type="text"
            bind:value={mfaCode}
            required
            inputmode="numeric"
            autocomplete="one-time-code"
          />
        </label>

        <label class="checkbox-label">
          <input type="checkbox" bind:checked={useBackupCode} />
          {t('auth.login.use_backup_code')}
        </label>

        <button type="submit" class="btn-primary" disabled={getAuthLoading() || !mfaCode.trim()}>
          {getAuthLoading() ? t('auth.login.verifying') : t('auth.login.verify')}
        </button>
      </form>
    {:else}
      <form onsubmit={handleSubmit}>
        <label>
          {t('auth.login.username')}
          <input type="text" bind:value={username} required autocomplete="username" />
        </label>

        <label>
          {t('auth.login.password')}
          <input type="password" bind:value={password} required autocomplete="current-password" />
        </label>

        <button type="submit" class="btn-primary" disabled={getAuthLoading()}>
          {getAuthLoading() ? t('auth.login.submitting') : t('auth.login.submit')}
        </button>
      </form>
    {/if}

    {#if ssoLoading}
      <p class="sso-loading">{t('auth.login.sso_loading')}</p>
    {:else if ssoProviders.length > 0 && !isMfaRequired()}
      <div class="sso-divider"><span>{t('auth.login.sso_or')}</span></div>
      <div class="sso-providers">
        {#each ssoProviders as provider (provider.slug)}
          <a class="sso-button" href={auth.ssoAuthorizeUrl(provider.slug)}>
            {#if provider.icon_url}
              <img src={provider.icon_url} alt="" width="20" height="20" />
            {/if}
            {t('auth.login.sso_continue', { provider: provider.name })}
          </a>
        {/each}
      </div>
    {/if}

    <p class="footer">
      {t('auth.login.footer', { link: '' })}
      <a href="/register">{t('auth.login.footer_link')}</a>
      <span class="separator">·</span>
      <a href="/forgot-password">{t('auth.login.forgot_password')}</a>
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

  .checkbox-label {
    flex-direction: row;
    font-weight: 500;
  }

  .checkbox-label input {
    width: auto;
  }

  .sso-loading {
    margin: 18px 0 0;
    color: var(--text-secondary);
    font-size: 13px;
    text-align: center;
  }

  .sso-divider {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 20px 0 14px;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .sso-divider::before,
  .sso-divider::after {
    content: '';
    flex: 1;
    border-top: 1px solid var(--border);
  }

  .sso-providers { display: grid; gap: 10px; }

  .sso-button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    text-decoration: none;
  }

  .sso-button:hover { background: var(--bg-tertiary); }
  .sso-button img { object-fit: contain; }

  .footer {
    text-align: center;
    margin-top: 20px;
    font-size: 13px;
    color: var(--text-secondary);
  }
</style>
