<script lang="ts">
  // Blob breadcrumb — repo name + path segments, each linking into the tree
  // browser. Pure presentation.
  import { buildTreeHref } from '$lib/utils/repoUrls';

  interface Props {
    owner: string;
    repo: string;
    ref: string;
    filePath: string;
  }

  let { owner, repo, ref, filePath }: Props = $props();

  const segments = $derived.by(() => {
    const parts = filePath.split('/');
    const crumbs: { name: string; href: string }[] = [];
    let accumulated = '';
    for (let i = 0; i < parts.length - 1; i += 1) {
      accumulated = accumulated ? `${accumulated}/${parts[i]}` : parts[i];
      crumbs.push({ name: parts[i], href: buildTreeHref(owner, repo, ref, accumulated) });
    }
    return crumbs;
  });
</script>

<div class="blob-breadcrumb">
  <a href={buildTreeHref(owner, repo, ref, '')} class="crumb-link">{repo}</a>
  {#each segments as crumb (crumb.name)}
    <span class="crumb-sep">/</span>
    <a href={crumb.href} class="crumb-link">{crumb.name}</a>
  {/each}
</div>

<style>
  .blob-breadcrumb {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 14px;
    margin-bottom: 12px;
  }

  .crumb-link {
    color: var(--accent);
    text-decoration: none;
  }

  .crumb-link:hover {
    text-decoration: underline;
  }

  .crumb-sep {
    color: var(--text-muted);
  }
</style>
