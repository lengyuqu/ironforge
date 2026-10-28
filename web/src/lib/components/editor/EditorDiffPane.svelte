<script lang="ts">
  import { createT } from '$lib/i18n';
  import { buildLineDiff } from '$lib/utils/diff';

  interface Props {
    original: string;
    current: string;
    /** False when there is nothing to show (edit mode with identical text). */
    changed: boolean;
  }

  let { original, current, changed }: Props = $props();

  const t = createT();

  let lines = $derived(buildLineDiff(original, current));
</script>

<div class="diff-pane">
  {#if changed}
    <table>
      <tbody>
        {#each lines as line}
          <tr class={line.type}>
            <td class="line-number">{line.oldNumber ?? ''}</td>
            <td class="line-number">{line.newNumber ?? ''}</td>
            <td class="marker">{line.type === 'add' ? '+' : line.type === 'del' ? '-' : ' '}</td>
            <td class="line-text">{line.text || ' '}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="muted">{t('repo.editor.no_changes')}</p>
  {/if}
</div>

<style>
  .diff-pane {
    min-height: 360px;
    max-height: 680px;
    overflow: auto;
    background: var(--bg-primary);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    font-size: 13px;
    line-height: 1.55;
  }

  tr.add {
    background: color-mix(in srgb, var(--green-dim) 13%, transparent);
  }

  tr.del {
    background: color-mix(in srgb, var(--red) 12%, transparent);
  }

  .line-number {
    width: 1%;
    min-width: 44px;
    padding: 0 8px;
    text-align: right;
    color: var(--text-muted);
    border-right: 1px solid var(--border-light);
    user-select: none;
    white-space: nowrap;
  }

  .marker {
    width: 1%;
    padding: 0 8px;
    color: var(--text-muted);
    user-select: none;
  }

  .line-text {
    padding: 0 12px 0 4px;
    white-space: pre;
  }

  .muted {
    margin: 0;
    padding: 16px;
    color: var(--text-muted);
  }
</style>
