<script lang="ts">
  interface Props {
    status: string;
  }

  let { status }: Props = $props();

  const statusConfig: Record<string, { label: string; color: string; bg: string }> = {
    pending:  { label: 'Pending',  color: 'var(--yellow)',  bg: 'rgba(210, 153, 34, 0.15)' },
    running:  { label: 'Running',  color: 'var(--accent)',  bg: 'rgba(88, 166, 255, 0.15)' },
    success:  { label: 'Passed',   color: 'var(--green)',   bg: 'rgba(63, 185, 80, 0.15)' },
    failed:   { label: 'Failed',   color: 'var(--red)',     bg: 'rgba(248, 81, 73, 0.15)' },
    canceled: { label: 'Canceled', color: 'var(--text-muted)', bg: 'rgba(110, 118, 129, 0.15)' },
  };

  const cfg = $derived(statusConfig[status] || statusConfig.pending);
</script>

<span class="badge" style="color: {cfg.color}; background: {cfg.bg}">
  {#if status === 'running'}
    <span class="spinner"></span>
  {:else if status === 'success'}
    ✓
  {:else if status === 'failed'}
    ✗
  {:else}
    ●
  {/if}
  {cfg.label}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 600;
  }

  .spinner {
    width: 10px;
    height: 10px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
