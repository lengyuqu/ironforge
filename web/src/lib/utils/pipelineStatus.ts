// Shared helpers for pipeline / job status display.
// Used by the pipelines page and its child components.

/** Human-readable duration between two ISO timestamps (e.g. "1m 42s"). */
export function formatDuration(start?: string | null, end?: string | null): string {
  if (!start) return '-';
  const s = new Date(start).getTime();
  const e = end ? new Date(end).getTime() : Date.now();
  const sec = Math.floor((e - s) / 1000);
  if (sec < 0) return '0s';
  if (sec < 60) return sec + 's';
  if (sec < 3600) return Math.floor(sec / 60) + 'm ' + (sec % 60) + 's';
  return Math.floor(sec / 3600) + 'h ' + Math.floor((sec % 3600) / 60) + 'm';
}

/** Glyph for a pipeline/job status. */
export function statusIcon(status: string): string {
  switch (status) {
    case 'success': return '✓';
    case 'failed': case 'error': return '✗';
    case 'running': return '⟳';
    case 'manual': return '▶';
    case 'waiting_approval': return '⏳';
    case 'canceled': return '−';
    case 'skipped': return '○';
    default: return '●';
  }
}

/** CSS color (var reference) for a pipeline/job status. */
export function statusColor(status: string): string {
  switch (status) {
    case 'success': return 'var(--green)';
    case 'failed': case 'error': return 'var(--red)';
    case 'running': return 'var(--accent)';
    case 'canceled': return 'var(--text-muted)';
    case 'skipped': return 'var(--text-muted)';
    default: return 'var(--yellow)';
  }
}

/** Whether a status is considered in-flight (auto-refresh eligible). */
export function isRunning(status: string): boolean {
  return status === 'running' || status === 'pending';
}
