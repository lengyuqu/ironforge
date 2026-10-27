<script lang="ts">
  import SuccessBanner from '$lib/components/SuccessBanner.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { onMount } from 'svelte';
  import { auth } from '$lib/api/client.svelte';
  import { setToken } from '$lib/api/client.svelte';
  import { fetchUser } from '$lib/stores/auth.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let token = $state('');
  let password = $state('');
  let confirmPassword = $state('');
  let loading = $state(false);
  let success = $state(false);
  let localError = $state('');

  const specialChars = /[!@#$%^&*()_+\-=[\]{}|;:,.<>?/~`'"\\]/;
  const passwordPattern =
    "(?=.*[a-z])(?=.*[A-Z])(?=.*\\d)(?=.*[!@#$%^&*()_+\\-=\\[\\]{}|;:,.<>?/~`'\\\"\\\\])\\S{8,128}";

  onMount(() => {
    const params = new URLSearchParams(window.location.search);
    const t = params.get('token');
    if (t) token = t;
  });

  async function handleSubmit(e: Event) {
    e.preventDefault();
    localError = '';

    if (password !== confirmPassword) {
      localError = 'Passwords do not match';
      return;
    }

    const passwordError = validatePassword(password);
    if (passwordError) {
      localError = passwordError;
      return;
    }

    loading = true;
    try {
      const res = await auth.resetPassword(token, password);
      setToken(res.token);
      await fetchUser();
      success = true;
    } catch (e: any) {
      localError = e.message || t('errors.reset_failed');
    } finally {
      loading = false;
    }
  }

  function validatePassword(value: string): string {
    if (value.length < 8) return 'Password must be at least 8 characters';
    if (value.length > 128) return 'Password must be at most 128 characters';
    if (/\s/.test(value)) return 'Password must not contain spaces';
    if (!/[A-Z]/.test(value)) return 'Password must contain an uppercase letter';
    if (!/[a-z]/.test(value)) return 'Password must contain a lowercase letter';
    if (!/[0-9]/.test(value)) return 'Password must contain a number';
    if (!specialChars.test(value)) return 'Password must contain a special character';
    return '';
  }
</script>

<div class="reset-page">
  <div class="reset-card">
    <div class="reset-header">
      <h1>Reset Password</h1>
      <p class="subtitle">Enter your new password</p>
    </div>

    {#if success}
      <SuccessBanner message={t('auth.reset.success')} />
      <a href="/dashboard" class="btn-secondary" style="display:block;text-align:center;margin-top:16px;">
        {t('auth.reset.go_dashboard')}
      </a>
    {:else if !token}
      <ErrorBanner message={t('auth.reset.invalid_link')} />
      <a href="/forgot-password" class="btn-secondary" style="display:block;text-align:center;margin-top:16px;">
        Request Reset
      </a>
    {:else}
      {#if localError}
        <ErrorBanner message={localError} />
      {/if}

      <form onsubmit={handleSubmit}>
        <label>
          New Password
          <input
            type="password"
            bind:value={password}
            required
            minlength={8}
            maxlength={128}
            pattern={passwordPattern}
            placeholder="At least 8 characters"
            autocomplete="new-password"
          />
        </label>

        <label>
          Confirm Password
          <input
            type="password"
            bind:value={confirmPassword}
            required
            placeholder="Re-enter password"
            autocomplete="new-password"
          />
        </label>

        <button type="submit" class="btn-primary" disabled={loading}>
          {loading ? 'Resetting...' : 'Reset Password'}
        </button>
      </form>

      <p class="footer">
        <a href="/login">Back to Login</a>
      </p>
    {/if}
  </div>
</div>

<style>
  .reset-card {
    background: var(--card-bg, #fff);
    border: 1px solid var(--border, #e5e7eb);
    border-radius: 8px;
    padding: 32px;
    width: 100%;
    max-width: 400px;
  }
  .reset-header {
    text-align: center;
    margin-bottom: 24px;
  }
  .reset-header h1 {
    margin: 0 0 8px;
    font-size: 24px;
    color: var(--text, #1f2937);
  }
  .subtitle {
    margin: 0;
    color: var(--text-muted, #6b7280);
    font-size: 14px;
  }
  label {
    display: block;
    margin-bottom: 16px;
    font-weight: 500;
    color: var(--text, #1f2937);
  }
  input {
    display: block;
    width: 100%;
    margin-top: 4px;
    padding: 8px 12px;
    border: 1px solid var(--border, #d1d5db);
    border-radius: 6px;
    font-size: 14px;
    box-sizing: border-box;
  }
  .footer {
    text-align: center;
    margin-top: 16px;
    font-size: 14px;
  }
  .footer a {
    color: var(--accent, #4f46e5);
    text-decoration: none;
  }
</style>
