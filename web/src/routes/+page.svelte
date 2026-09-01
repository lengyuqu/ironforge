<script lang="ts">
  // Home page — two branches:
  //   logged in  → dashboard with the user's repo grid
  //   logged out → marketing landing (hero / features / stats / public repos)
  // Heavy UI lives in $lib/components/home/.
  import { isLoggedIn } from '$lib/stores/auth.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import HeroSection from '$lib/components/home/HeroSection.svelte';
  import FeaturesSection from '$lib/components/home/FeaturesSection.svelte';
  import StatsSection from '$lib/components/home/StatsSection.svelte';
  import PublicReposSection from '$lib/components/home/PublicReposSection.svelte';
  import SiteFooter from '$lib/components/home/SiteFooter.svelte';
  import DashboardRepoGrid from '$lib/components/home/DashboardRepoGrid.svelte';
  import type { ExploreRepo } from '$lib/types/entities';

  const t = createT();

  let repoList = $state<ExploreRepo[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    loadRepos();
  });

  async function loadRepos() {
    try {
      loading = true;
      error = '';
      const r = await repos.explore(1, 6);
      if (r && Array.isArray(r.data)) {
        repoList = r.data;
      }
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>IronForge</title>
</svelte:head>

{#if isLoggedIn()}
  <!-- User Dashboard -->
  <div class="dashboard">
    <div class="dashboard-header">
      <h1>{t('dashboard.title')}</h1>
      <button class="btn-primary" onclick={() => goto('/dashboard')}>
        + {t('dashboard.new_repo')}
      </button>
    </div>

    {#if error}
      <div class="error-banner">{error}</div>
    {/if}

    <DashboardRepoGrid repos={repoList} {loading} />

    <div class="dashboard-footer">
      <a href="/explore" class="view-all-btn">{t('home.explore.view_all')} →</a>
    </div>
  </div>
{:else}
  <!-- Product Landing Page -->
  <div class="landing">
    <HeroSection />
    <FeaturesSection />
    <StatsSection />
    <PublicReposSection repos={repoList} {loading} {error} onRetry={loadRepos} />
    <SiteFooter />
  </div>
{/if}

<style>
  .landing {
    min-height: 100vh;
  }

  /* === Dashboard Styles (Logged In) === */
  .dashboard {
    max-width: 1200px;
    margin: 0 auto;
    padding: 24px;
  }

  .dashboard-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

  .dashboard-header h1 {
    font-size: 24px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--green-dim, #2da44e);
    color: #fff;
    border: none;
    border-radius: var(--radius, 6px);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:hover {
    background: var(--green, #218838);
  }

  .dashboard-footer {
    text-align: center;
  }

  .view-all-btn {
    display: inline-block;
    padding: 8px 20px;
    background: var(--primary, #2da44e);
    color: white;
    border-radius: 6px;
    text-decoration: none;
    font-size: 14px;
  }
</style>
