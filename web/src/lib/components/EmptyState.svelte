<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    /** One-line statement of what is missing, e.g. "No issues yet". */
    title: string;
    /** Optional hint on what to do next. */
    description?: string;
    /** Decorative glyph above the title. */
    icon?: string;
    /** Call-to-action (button or link) rendered under the text. */
    children?: Snippet;
  }

  let { title, description, icon, children }: Props = $props();
</script>

<div class="empty-state-box">
  {#if icon}<div class="empty-icon" aria-hidden="true">{icon}</div>{/if}
  <p class="empty-title">{title}</p>
  {#if description}<p class="empty-description">{description}</p>{/if}
  {#if children}<div class="empty-actions">{@render children()}</div>{/if}
</div>

<style>
  .empty-state-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 40px 16px;
    text-align: center;
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    background: var(--bg-secondary);
  }

  .empty-icon {
    font-size: 28px;
    line-height: 1;
    margin-bottom: 4px;
    opacity: 0.8;
  }

  .empty-title {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .empty-description {
    margin: 0;
    max-width: 420px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .empty-actions {
    margin-top: 10px;
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: center;
  }
</style>
