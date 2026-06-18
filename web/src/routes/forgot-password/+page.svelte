<script lang="ts">
  import { auth } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let email = $state('');
  let loading = $state(false);
  let success = $state(false);
  let localError = $state('');

  async function handleSubmit(e: Event) {
    e.preventDefault();
    localError = '';
    loading = true;
    try {
      await auth.forgotPassword(email);
      success = true;
    } catch (e: any) {
      localError = e.message || 'Failed to send reset email';
    } finally {
      loading = false;
    }
  }
</script>

<div class="reset-page">
  <div class="reset-card">
    <div class="reset-header">
      <h1>Forgot Password</h1>
      <p class="subtitle">Enter your email to receive a password reset link</p>
    </div>

    {#if success}
      <div class="success-banner">
        If the email exists, a reset link has been sent. Please check your inbox.
      </div>
      <a href="/login" class="btn-secondary" style="display:block;text-align:center;margin-top:16px;">
        Back to Login
      </a>
    {:else}
      {#if localError}
        <div class="error-banner">{localError}</div>
      {/if}

      <form onsubmit={handleSubmit}>
        <label>
          Email
          <input
            type="email"
            bind:value={email}
            required
            placeholder="you@example.com"
            autocomplete="email"
          />
        </label>

        <button type="submit" class="btn-primary" disabled={loading}>
          {loading ? 'Sending...' : 'Send Reset Link'}
        </button>
      </form>

      <p class="footer">
        <a href="/login">Back to Login</a>
      </p>
    {/if}
  </div>
</div>

<style>
  .reset-page {
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 60vh;
    padding: 20px;
  }
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
  .btn-primary {
    width: 100%;
    padding: 10px 16px;
    background: var(--accent, #4f46e5);
    color: #fff;
    border: none;
    border-radius: 6px;
    font-size: 14px;
    cursor: pointer;
  }
  .btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .btn-secondary {
    padding: 10px 16px;
    background: var(--bg-secondary, #f3f4f6);
    color: var(--text, #1f2937);
    border: 1px solid var(--border, #d1d5db);
    border-radius: 6px;
    font-size: 14px;
    text-decoration: none;
  }
  .error-banner {
    background: #fef2f2;
    border: 1px solid #fecaca;
    color: #dc2626;
    padding: 10px 14px;
    border-radius: 6px;
    margin-bottom: 16px;
    font-size: 14px;
  }
  .success-banner {
    background: #f0fdf4;
    border: 1px solid #bbf7d0;
    color: #16a34a;
    padding: 14px 18px;
    border-radius: 6px;
    font-size: 14px;
    line-height: 1.5;
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
