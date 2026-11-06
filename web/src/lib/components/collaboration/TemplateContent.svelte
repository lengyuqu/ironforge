<script lang="ts">
  // One template version rendered as inert text. Template text is written by a
  // repository admin (or whatever tool uses an admin's account), so it is
  // displayed, never interpreted: no markup, no links, no instructions.
  import { createT } from '$lib/i18n';
  import type { TemplateContent } from '$lib/api/collabOps';

  interface Props {
    content: TemplateContent;
  }
  let { content }: Props = $props();
  const t = createT();
</script>

<dl class="tc" data-testid="template-content">
  <dt>{t('collab_ops.templates.title_pattern')}</dt>
  <dd class="mono text">{content.title_pattern}</dd>
  <dt>{t('collab_ops.templates.goal')}</dt>
  <dd class="text pre">{content.goal}</dd>
  <dt>{t('collab_ops.templates.deliverables')}</dt>
  <dd>
    {#if content.deliverables.length === 0}
      <span class="muted">{t('collab_ops.none')}</span>
    {:else}
      <ul>{#each content.deliverables as d}<li class="text">{d}</li>{/each}</ul>
    {/if}
  </dd>
  <dt>{t('collab_ops.templates.checklist')}</dt>
  <dd>
    {#if content.checklist.length === 0}
      <span class="muted">{t('collab_ops.none')}</span>
    {:else}
      <ul>
        {#each content.checklist as c}
          <li class="text">{c.text}{#if !c.required} <span class="muted">({t('collab_ops.templates.optional_item')})</span>{/if}</li>
        {/each}
      </ul>
      <p class="muted">{t('collab_ops.templates.checklist_note')}</p>
    {/if}
  </dd>
</dl>

<style>
  .tc { display: grid; grid-template-columns: fit-content(55%) minmax(0, 1fr); overflow-wrap: anywhere; gap: 6px 16px; margin: 8px 0; font-size: 13px; }
  dt { color: var(--text-secondary); font-weight: 600; }
  dd { margin: 0; min-width: 0; }
  ul { margin: 0; padding-left: 18px; }
  .text { overflow-wrap: anywhere; }
  .pre { white-space: pre-wrap; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0 0; }
  @media (max-width: 600px) { .tc { grid-template-columns: 1fr; } }
</style>
