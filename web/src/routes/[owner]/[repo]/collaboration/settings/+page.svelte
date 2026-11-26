<script lang="ts">
  // Three separate sections, each with its own load and its own failure state:
  // escalation settings (admin-managed), the sessions' self-reported capability
  // declarations (claims, never permissions) and a read-only deletion preview.
  // The API decides who may read or write each one.
  import { page } from '$app/state';
  import { createT } from '$lib/i18n';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';
  import EscalationSettings from '$lib/components/collaboration/EscalationSettings.svelte';
  import DeclarationsCard from '$lib/components/collaboration/DeclarationsCard.svelte';
  import DeletionImpactCard from '$lib/components/collaboration/DeletionImpactCard.svelte';

  const t = createT();
  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);
  let admin = $state<boolean | null>(null);
</script>

<svelte:head>
  <title>{t('collab_ops.settings.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<CollabNav {owner} {repo} current="settings" />

<div class="page-header">
  <h1>{t('collab_ops.settings.title')}</h1>
</div>
<p class="intro">{t('collab_ops.settings.intro')}</p>

<EscalationSettings {owner} {repo} knownNotAdmin={admin === false} />
<DeclarationsCard {owner} {repo} />
<DeletionImpactCard {owner} {repo} bind:admin />

<style>
  .intro { margin: -8px 0 20px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
</style>
