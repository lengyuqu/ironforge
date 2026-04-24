<script lang="ts">
  import '$lib/app.css';
  import Navbar from '$lib/components/Navbar.svelte';
  import { fetchUser } from '$lib/stores/auth';
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  // Fetch user on first load
  fetchUser();
</script>

<div class="app">
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
