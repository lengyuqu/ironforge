<script lang="ts">
  import { goto } from '$app/navigation';
  import { tokens } from '$lib/api/client.svelte';
  import type { AccessToken } from '$lib/api/client.svelte';
  import { isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import TokenCreateForm from '$lib/components/settings/TokenCreateForm.svelte';
  import TokenList from '$lib/components/settings/TokenList.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let tokenList = $state<AccessToken[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    loadTokens();
  });

  async function loadTokens() {
    try {
      loading = true;
      error = '';
      tokenList = await tokens.list();
    } catch (err: any) {
      error = err.message || t('errors.load_failed');
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>Access Tokens · IronForge</title>
</svelte:head>

<div class="page-container tokens-page">
  <header class="page-header">
    <div>
      <h1>{t('settings.tokens.title', 'Access Tokens')}</h1>
      <p>{t('settings.tokens.subtitle', 'Manage personal tokens for Git over HTTP, API clients, and automation.')}</p>
    </div>
  </header>

  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  <TokenCreateForm onCreated={loadTokens} />

  <TokenList tokenList={tokenList} {loading} onRefresh={loadTokens} />
</div>

<style>
  .tokens-page {
    max-width: 980px;
  }

  .page-header {
    margin-bottom: 24px;
  }

  h1 {
    margin: 0 0 6px;
    font-size: 28px;
  }

  p {
    margin: 0;
    color: var(--text-secondary);
  }

  .error-box {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 10%, transparent);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 16px;
    margin-bottom: 20px;
  }
</style>
