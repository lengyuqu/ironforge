<script lang="ts">
  import '$lib/app.css';
  import Navbar from '$lib/components/Navbar.svelte';
  import InstanceBanner from '$lib/components/InstanceBanner.svelte';
  import Layout from '$lib/components/Layout.svelte';
import { fetchUser, isAuthReady } from '$lib/stores/auth.svelte';
import { registerKeyboardShortcuts } from '$lib/stores/instance.svelte';
import { locale, createT } from '$lib/i18n';
import { onMount } from 'svelte';
import type { Snippet } from 'svelte';
import { setBanner } from '$lib/stores/instance.svelte';
import { withBackendBase } from '$lib/api/_base.svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  const t = createT();

  // Initialize i18n on first load
  locale.init();

  // Register global keyboard shortcuts and fetch user on mount
  onMount(() => {
    fetchUser();
    const unregister = registerKeyboardShortcuts();
    checkBackendReadiness();
    return unregister;
  });

  async function checkBackendReadiness() {
    try {
      const res = await fetch(withBackendBase('/health'), { cache: 'no-store' });
      if (!res.ok) {
        setBanner(t('system.backend_health_failed', { status: res.status }), 'error');
        return;
      }
      const body = await res.json().catch(() => null);
      if (!body || !['healthy', 'ok'].includes(String(body.status || ''))) {
        setBanner(t('system.backend_unhealthy'), 'warning');
        return;
      }
    } catch {
      setBanner(t('system.backend_unreachable'), 'error');
    }
  }
</script>

<div class="app">
  <InstanceBanner />
  <Navbar />
  <Layout>
    <main>
      {#if isAuthReady()}
        {@render children()}
      {:else}
        <div class="loading">{t('common.loading')}</div>
      {/if}
    </main>
  </Layout>
</div>

<style>
  .app {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  main {
    flex: 1;
  }
</style>
