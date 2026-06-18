<script lang="ts">
  import '$lib/app.css';
  import Navbar from '$lib/components/Navbar.svelte';
  import InstanceBanner from '$lib/components/InstanceBanner.svelte';
  import { fetchUser } from '$lib/stores/auth.svelte';
  import { registerKeyboardShortcuts } from '$lib/stores/instance.svelte';
  import { locale } from '$lib/i18n';
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  // Initialize i18n and fetch user on first load
  locale.init();
  fetchUser();

  // Register global keyboard shortcuts
  onMount(() => {
    return registerKeyboardShortcuts();
  });
</script>

<div class="app">
  <InstanceBanner />
  <Navbar />
  <main>
    {@render children()}
  </main>
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
