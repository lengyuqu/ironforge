<script lang="ts">
  // Home page — two branches:
  //   logged in  → dashboard with the signed-in user's own repositories
  //   logged out → marketing landing (hero / features / stats / public repos)
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { getUser, isLoggedIn } from '$lib/stores/auth.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { toErrorMessage } from '$lib/utils/error';
  import HeroSection from '$lib/components/home/HeroSection.svelte';
  import FeaturesSection from '$lib/components/home/FeaturesSection.svelte';
  import StatsSection from '$lib/components/home/StatsSection.svelte';
  import PublicReposSection from '$lib/components/home/PublicReposSection.svelte';
  import SiteFooter from '$lib/components/home/SiteFooter.svelte';
  import DashboardRepoGrid from '$lib/components/home/DashboardRepoGrid.svelte';
  import type { ExploreRepo } from '$lib/types/entities';

  const t = createT();
  const latest = createLatest();

  let repoList = $state<ExploreRepo[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    void loadRepos();
  });

  async function loadRepos() {
    const currentUser = isLoggedIn() ? getUser() : null;
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      if (currentUser?.username) {
        const res = await repos.list(currentUser.username, 1, 6);
        if (!isCurrent()) return;
        repoList = (res?.data || []).map((item) => ({
          ...item,
          owner_id: currentUser.id,
          owner_name: currentUser.username,
        }));
      } else {
        const r = await repos.explore(1, 6);
        if (!isCurrent()) return;
        repoList = r && Array.isArray(r.data) ? r.data : [];
      }
    } catch (e: unknown) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }
</script>

<svelte:head>
  <title>IronForge</title>
</svelte:head>

{#if isLoggedIn()}
  <!-- User Dashboard -->
  <div class="page-container">
    <div class="dashboard-header">
      <h1>{t('dashboard.title')}</h1>
      <div class="dashboard-actions">
        <a href="/dashboard" class="btn-outline">{t('home.cta.go_dashboard')}</a>
        <button class="btn-primary" onclick={() => goto('/dashboard?new=1')}>
          + {t('dashboard.new_repo')}
        </button>
      </div>
    </div>

    {#if error}
      <ErrorBanner message={error} />
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
  .dashboard-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 24px;
  }

  .dashboard-header h1 {
    font-size: 24px;
  }

  .dashboard-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dashboard-footer {
    text-align: center;
  }

  .view-all-btn {
    display: inline-block;
    padding: 8px 20px;
    background: var(--green-dim);
    color: var(--text-on-accent);
    border-radius: 6px;
    text-decoration: none;
    font-size: 14px;
  }
</style>
