<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
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
    } catch (err) {
      error = toErrorMessage(err, t('errors.load_failed'));
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>{t('settings.tokens.title')} · IronForge</title>
</svelte:head>

<div class="page-container tokens-page">
  <header class="page-header">
    <div>
      <h1>{t('settings.tokens.title', 'Access Tokens')}</h1>
      <p>{t('settings.tokens.subtitle', 'Manage personal tokens for Git over HTTP, API clients, and automation.')}</p>
    </div>
  </header>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  <TokenCreateForm onCreated={loadTokens} />

  <TokenList tokenList={tokenList} {loading} onRefresh={loadTokens} />
</div>

<style>
  .tokens-page {
    max-width: 980px;
  }

  h1 {
    margin: 0 0 6px;
  }

  p {
    margin: 0;
    color: var(--text-secondary);
  }

</style>
