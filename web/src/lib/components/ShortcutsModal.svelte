<script lang="ts">
  import Modal from './Modal.svelte';
  import { createT } from '$lib/i18n';

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();
  const t = createT();

  const items = $derived([
    { keys: ['/', 'Ctrl+K'], label: t('shortcuts.focus_search') },
    { keys: ['?'], label: t('shortcuts.toggle_help') },
    { keys: ['g', 'd'], label: t('shortcuts.go_dashboard') },
    { keys: ['g', 'e'], label: t('shortcuts.go_explore') },
    { keys: ['g', 'n'], label: t('shortcuts.go_notifications') },
    { keys: ['g', 's'], label: t('shortcuts.go_search') },
  ]);
</script>

<Modal {open} {onClose} title={t('shortcuts.title')}>
  <ul class="shortcuts-list">
    {#each items as item (item.label)}
      <li class="shortcut-row">
        <span class="shortcut-label">{item.label}</span>
        <span class="shortcut-keys">
          {#each item.keys as k, idx (k)}
            {#if idx > 0}<span class="sep">{item.keys[0] === 'g' ? '→' : '/'}</span>{/if}
            <kbd>{k}</kbd>
          {/each}
        </span>
      </li>
    {/each}
  </ul>
</Modal>

<style>
  .shortcuts-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .shortcut-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 0;
    border-bottom: 1px solid var(--border-light);
    font-size: 13px;
  }

  .shortcut-row:last-child {
    border-bottom: none;
  }

  .shortcut-label {
    color: var(--text-primary);
  }

  .shortcut-keys {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .sep {
    color: var(--text-muted);
    font-size: 12px;
  }

  kbd {
    display: inline-block;
    padding: 2px 6px;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-primary);
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
  }
</style>
