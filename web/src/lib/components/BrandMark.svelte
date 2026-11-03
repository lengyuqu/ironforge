<script lang="ts">
  // IronForge mark: a git branch rising from an anvil. Source of truth for the
  // geometry is assets/brand/ironforge-mark.svg; keep the two in sync.
  //
  // The anvil follows `currentColor`; the branch uses `--brand-ember` so the
  // mark works on any background the parent sets a text colour for.

  /** Size in pixels. Defaults to 32. */
  /** Draw the branch in `currentColor` too (single-colour contexts). */
  let { size = 32, mono = false }: { size?: number; mono?: boolean } = $props();
</script>

<svg
  class="brand-mark"
  class:mono
  width={size}
  height={size}
  viewBox="0 0 32 32"
  xmlns="http://www.w3.org/2000/svg"
  aria-hidden="true"
>
  <path
    fill="currentColor"
    d="M3 14H29V18H25.5C23.6 18 22.5 19.4 22.5 21.2V23.5H25C25.8 23.5 26.5 24.2 26.5 25V28H9.5V25C9.5 24.2 10.2 23.5 11 23.5H13.5V21.2C13.5 19.4 12.4 18 10.5 18H9.5C6.5 18 4.2 16.6 3 14Z"
  />
  <g class="ember" stroke-width="2.6" stroke-linecap="round">
    <path fill="none" d="M13 14V8M13 12.5C13 9.5 21 11.2 21 8.3" />
    <circle cx="13" cy="5.5" r="2.7" stroke="none" />
    <circle cx="21" cy="6" r="2.7" stroke="none" />
  </g>
</svg>

<style>
  .brand-mark {
    flex-shrink: 0;
  }

  .ember {
    fill: var(--brand-ember);
    stroke: var(--brand-ember);
  }

  .mono .ember {
    fill: currentColor;
    stroke: currentColor;
  }
</style>
