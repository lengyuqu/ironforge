/// Global instance state store — maintenance mode, banner, keyboard shortcuts.
/// Uses Svelte 5 runes ($state).

// ── Instance Banner ─────────────────────────────────────

let bannerMessage = $state('');
let bannerType = $state<'info' | 'warning' | 'error'>('info');

export function getBanner() {
  return { message: bannerMessage, type: bannerType };
}

export function setBanner(message: string, type: 'info' | 'warning' | 'error' = 'info') {
  bannerMessage = message;
  bannerType = type;
}

export function clearBanner() {
  bannerMessage = '';
}

// ── Keyboard Shortcuts ──────────────────────────────────

export interface KeyboardShortcutCallbacks {
  onToggleShortcutsHelp?: () => void;
  onNavigate?: (path: string) => void;
}

const CHORD_ROUTES: Record<string, string> = {
  d: '/dashboard',
  e: '/explore',
  n: '/notifications',
  s: '/search',
};

/// Call this once in root layout to register global keyboard shortcuts.
export function registerKeyboardShortcuts(callbacks: KeyboardShortcutCallbacks = {}) {
  if (typeof window === 'undefined') return;

  let awaitingChord = false;
  let chordTimer: ReturnType<typeof setTimeout> | null = null;

  function clearChord() {
    awaitingChord = false;
    if (chordTimer) {
      clearTimeout(chordTimer);
      chordTimer = null;
    }
  }

  function isEditable(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    if (!el) return false;
    const tag = el.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
  }

  function handler(e: KeyboardEvent) {
    // Ctrl/Cmd+K focuses global search from anywhere (unless another modifier is held).
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      clearChord();
      focusSearch();
      return;
    }

    if (isEditable(e.target)) {
      clearChord();
      return;
    }

    if (e.ctrlKey || e.metaKey || e.altKey) {
      clearChord();
      return;
    }

    // Two-key navigation chord: `g` then `d` / `e` / `n` / `s`
    if (awaitingChord) {
      const destination = CHORD_ROUTES[e.key.toLowerCase()];
      clearChord();
      if (destination) {
        e.preventDefault();
        callbacks.onNavigate?.(destination);
      }
      return;
    }

    if (e.key === 'g' && !e.shiftKey) {
      awaitingChord = true;
      chordTimer = setTimeout(clearChord, 1000);
      return;
    }

    // '/' — Focus global search
    if (e.key === '/') {
      e.preventDefault();
      focusSearch();
      return;
    }

    // '?' — Toggle keyboard shortcuts dialog (`?` requires Shift on standard keyboards)
    if (e.key === '?') {
      e.preventDefault();
      callbacks.onToggleShortcutsHelp?.();
    }
  }

  document.addEventListener('keydown', handler);
  return () => {
    clearChord();
    document.removeEventListener('keydown', handler);
  };
}

function focusSearch() {
  const searchInput =
    document.querySelector<HTMLInputElement>('.navbar-search .search-input') ??
    document.querySelector<HTMLInputElement>(
      'input[type="search"], input[placeholder*="earch"], input[placeholder*="搜索"]',
    );
  if (searchInput) {
    searchInput.focus();
    searchInput.select();
  }
}
