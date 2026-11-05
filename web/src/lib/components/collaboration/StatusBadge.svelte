<script lang="ts">
  // A small coloured chip for a DECLARED or DERIVED state value. The text is
  // always rendered as text; an unknown value (a newer server) is shown raw.
  import { createT } from '$lib/i18n';
  import { knownLabelKey } from '$lib/utils/collaboration';

  interface Props {
    value: string | null | undefined;
    /** i18n prefix of the known values, e.g. `collab.task_status`. */
    prefix: string;
    known: readonly string[];
    /** Values rendered in the positive / warning / negative colour. */
    good?: readonly string[];
    warn?: readonly string[];
    bad?: readonly string[];
    title?: string;
  }

  let { value, prefix, known, good = [], warn = [], bad = [], title }: Props = $props();
  const t = createT();

  let key = $derived(knownLabelKey(prefix, value, known));
  let tone = $derived(
    value && good.includes(value) ? 'good' : value && warn.includes(value) ? 'warn' : value && bad.includes(value) ? 'bad' : 'neutral',
  );
</script>

<span class="chip {tone}" {title}>{key ? t(key) : (value ?? '—')}</span>

<style>
  .chip {
    display: inline-block;
    padding: 1px 8px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    line-height: 18px;
    white-space: nowrap;
    overflow-wrap: anywhere;
  }
  .good { color: var(--green); background: var(--green-subtle); border-color: var(--green-dim); }
  .warn { color: var(--yellow); background: var(--yellow-subtle); border-color: var(--yellow-dim); }
  .bad { color: var(--red); background: var(--red-subtle); border-color: var(--red-dim); }
</style>
