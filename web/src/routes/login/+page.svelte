<script lang="ts">
  import {
    login,
    verifyMfa,
    getAuthError,
    getAuthLoading,
    isLoggedIn,
    isMfaRequired,
  } from '$lib/stores/auth.svelte';
  import { auth, type PublicSsoProvider } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
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
    loadSsoProviders();
  });

  async function loadSsoProviders() {
    try {
      ssoLoading = true;
      ssoProviders = await auth.listSsoProviders();
    } catch {
      ssoProviders = [];
    } finally {
      ssoLoading = false;
    }
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    localError = '';
    const ok = await login(username, password);
    if (ok) {
      window.location.href = '/dashboard';
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
      window.location.href = '/dashboard';
    } else {
      localError = getAuthError() || t('auth.login.mfa_failed');
    }
  }
</script>

<svelte:head>
  <title>{t('auth.login.title')} · IronForge</title>
</svelte:head>

<div class="login-page">
  <div class="login-card">
    <div class="login-header">
      <svg viewBox="0 0 16 16" width="40" height="40" fill="var(--accent)">
        <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
      </svg>
      <h1>{t('auth.login.title')}</h1>
    </div>

    {#if localError}
      <div class="error-banner">{localError}</div>
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

      {#if !ssoLoading && ssoProviders.length > 0}
        <div class="sso-section">
          <div class="divider"><span>or</span></div>
          <div class="sso-buttons">
            {#each ssoProviders as provider (provider.slug)}
              <a class="sso-button" href={auth.ssoAuthorizeUrl(provider.slug)}>
                {#if provider.icon_url}
                  <img src={provider.icon_url} alt="" />
                {/if}
                <span>Continue with {provider.name}</span>
              </a>
            {/each}
          </div>
        </div>
      {/if}
    {/if}

    <p class="footer">
      {t('auth.login.footer', { link: '' })}
      <a href="/register">{t('auth.login.footer_link')}</a>
      <span class="separator">·</span>
      <a href="/forgot-password">Forgot password?</a>
    </p>
  </div>
</div>

<style>

  .login-card {
    width: 340px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 32px;
  }

  .login-header {
    text-align: center;
    margin-bottom: 24px;
  }

  h1 {
    font-size: 20px;
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

  input {
    padding: 8px 12px;
  }

  .checkbox-label {
    flex-direction: row;
    align-items: center;
    font-weight: 500;
  }

  .checkbox-label input {
    width: auto;
  }

  .btn-primary {
    padding: 8px 16px;
    background: var(--green-dim);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover { background: var(--green); }
  .btn-primary:disabled { opacity: 0.6; }

  .sso-section {
    margin-top: 20px;
  }

  .divider {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
    color: var(--text-secondary);
    font-size: 12px;
    text-transform: uppercase;
  }

  .divider::before,
  .divider::after {
    content: '';
    flex: 1;
    height: 1px;
    background: var(--border);
  }

  .sso-buttons {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .sso-button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 36px;
    padding: 7px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    background: var(--bg-primary);
    font-size: 14px;
    font-weight: 600;
    text-decoration: none;
  }

  .sso-button:hover {
    border-color: var(--accent);
    background: var(--bg-hover);
    text-decoration: none;
  }

  .sso-button img {
    width: 18px;
    height: 18px;
    object-fit: contain;
    flex-shrink: 0;
  }

  .sso-button span {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .footer {
    text-align: center;
    margin-top: 20px;
    font-size: 13px;
    color: var(--text-secondary);
  }
</style>
