// Commit status presentation helpers — shared by the commit detail page and
// any component rendering status checks.

export function statusIcon(state: string): string {
  switch (state) {
    case 'success':
      return '✅';
    case 'failure':
      return '❌';
    case 'error':
      return '❌';
    case 'pending':
      return '⏳';
    default:
      return '❓';
  }
}

export function statusText(state: string): string {
  switch (state) {
    case 'success':
      return 'All checks passed';
    case 'failure':
      return 'Some checks failed';
    case 'error':
      return 'Some checks errored';
    case 'pending':
      return 'Checks pending';
    default:
      return 'Unknown status';
  }
}

export function statusColor(state: string): string {
  switch (state) {
    case 'success':
      return 'var(--green)';
    case 'failure':
      return 'var(--red)';
    case 'error':
      return 'var(--orange)';
    case 'pending':
      return 'var(--yellow)';
    default:
      return 'var(--text-muted)';
  }
}
