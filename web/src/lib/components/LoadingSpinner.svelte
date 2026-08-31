<script lang="ts">
  /** Size in pixels. Defaults to 20. */
  /** Extra CSS class name (e.g. `inline`, `block`) for styling. */
  let { size = 20, className = '' }: { size?: number; className?: string } = $props();
</script>

<span class="spinner {className}" role="status" aria-label="Loading">
  <svg
    width={size}
    height={size}
    viewBox="0 0 50 50"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <circle
      cx="25"
      cy="25"
      r="20"
      fill="none"
      stroke="currentColor"
      stroke-opacity="0.25"
      stroke-width="4"
    />
    <circle
      cx="25"
      cy="25"
      r="20"
      fill="none"
      stroke="currentColor"
      stroke-width="4"
      stroke-linecap="round"
      stroke-dasharray="80, 200"
    />
  </svg>
</span>

<style>
  .spinner {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
  }

  .spinner svg circle:last-child {
    transform-origin: center;
    animation: spinner-rotate 1.2s linear infinite;
  }

  @keyframes spinner-rotate {
    100% { transform: rotate(360deg); }
  }

  .spinner.block {
    display: flex;
    width: 100%;
    padding: 24px 0;
  }

  .spinner.inline {
    vertical-align: -2px;
    margin-right: 6px;
  }
</style>
