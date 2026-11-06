<script lang="ts">
  // Links to the repository-level collaboration pages. The pages themselves are
  // separate routes; this list is the only place that names them all.
  import { createT } from '$lib/i18n';

  interface Props {
    owner: string;
    repo: string;
    /** Which page is showing; it is rendered as the current one. */
    current?: string;
  }

  let { owner, repo, current = 'home' }: Props = $props();
  const t = createT();

  const ITEMS = [
    { key: 'home', path: '' },
    { key: 'groups', path: '/groups' },
    { key: 'health', path: '/health' },
    { key: 'templates', path: '/templates' },
    { key: 'search', path: '/search' },
    { key: 'rules', path: '/rules' },
    { key: 'settings', path: '/settings' },
  ] as const;

  const href = (path: string) => `/${owner}/${repo}/collaboration${path}`;
</script>

<nav class="collab-nav" aria-label={t('collab.nav.label')} data-testid="collab-nav">
  {#each ITEMS as item (item.key)}
    <a
      href={href(item.path)}
      class:current={current === item.key}
      aria-current={current === item.key ? 'page' : undefined}
    >{t(`collab.nav.${item.key}`)}</a>
  {/each}
</nav>

<style>
  .collab-nav { display: flex; gap: 4px; flex-wrap: wrap; margin: 0 0 16px; border-bottom: 1px solid var(--border); }
  a {
    padding: 8px 12px; font-size: 14px; color: var(--text-secondary); text-decoration: none;
    border-bottom: 2px solid transparent; margin-bottom: -1px;
  }
  a:hover { color: var(--text-primary); }
  a.current { color: var(--text-primary); border-bottom-color: var(--accent, var(--green)); font-weight: 600; }
</style>
