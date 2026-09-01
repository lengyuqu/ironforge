<script lang="ts">
  // PR timeline — presentational list of timeline events (opened / merged /
  // review submitted / comment added ...).
  import { createT, formatDate } from '$lib/i18n';
  import type { PrTimelineEvent } from '$lib/types/entities';

  interface Props {
    events: PrTimelineEvent[];
  }

  let { events }: Props = $props();

  const t = createT();
</script>

<section class="timeline">
  <h3>{t('pulls.timeline.title')}</h3>
  {#each events as event (event.id)}
    <article class="timeline-event">
      <span class="timeline-dot"></span>
      <div>
        <div class="timeline-summary">
          <strong>{event.actor?.username || t('pulls.timeline.system')}</strong>
          <span>{t(`pulls.timeline.${event.kind}`, event.metadata || {})}</span>
          <time>{formatDate(event.created_at)}</time>
        </div>
        {#if event.metadata?.path}
          <code>{event.metadata.path}{event.metadata.line ? `:${event.metadata.start_line && event.metadata.start_line !== event.metadata.line ? `${event.metadata.start_line}-${event.metadata.line}` : event.metadata.line}` : ''}</code>
        {/if}
        {#if event.body}<div class="timeline-body">{event.body}</div>{/if}
      </div>
    </article>
  {/each}
</section>

<style>
  .timeline {
    margin-bottom: 16px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  h3 { font-size: 16px; margin-bottom: 12px; }

  .timeline-event {
    display: grid;
    grid-template-columns: 14px 1fr;
    gap: 10px;
    padding: 10px 0;
    border-top: 1px solid var(--border);
  }
  .timeline-event:first-of-type { border-top: none; }

  .timeline-dot {
    width: 9px;
    height: 9px;
    margin-top: 6px;
    border-radius: 50%;
    background: var(--accent);
  }

  .timeline-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    align-items: baseline;
  }
  .timeline-summary time {
    margin-left: auto;
    color: var(--text-secondary);
    font-size: 12px;
  }
  .timeline-event code { display: inline-block; margin-top: 5px; }

  .timeline-body {
    margin-top: 7px;
    white-space: pre-wrap;
    color: var(--text-secondary);
  }
</style>
