import { beforeNavigate, goto } from '$app/navigation';
import { confirmDialog } from '$lib/components/confirm.svelte';
import { createT } from '$lib/i18n';

export interface UnsavedChangesGuard {
  suspend<T>(fn: () => Promise<T>): Promise<T>;
}

/**
 * Warn before navigating away while a form or editor has unsaved changes.
 * Must be called during component initialization (`beforeNavigate` uses `onMount`).
 */
export function useUnsavedChangesGuard(isDirty: () => boolean): UnsavedChangesGuard {
  const t = createT();
  let suspended = 0;
  let prompting = false;

  beforeNavigate((nav) => {
    if (suspended > 0 || !isDirty()) return;
    nav.cancel();
    if (nav.willUnload) return; // tab close/reload/external: browser shows native prompt
    const target = nav.to?.url;
    if (!target || prompting) return;
    prompting = true;
    void confirmDialog({
      title: t('common.unsaved_title'),
      message: t('common.unsaved_message'),
      confirmLabel: t('common.unsaved_leave'),
      cancelLabel: t('common.unsaved_stay'),
      variant: 'danger',
    })
      .then(async (leave) => {
        if (!leave) return;
        suspended++;
        try {
          await goto(target.pathname + target.search + target.hash);
        } finally {
          suspended--;
        }
      })
      .finally(() => {
        prompting = false;
      });
  });

  return {
    async suspend(fn) {
      suspended++;
      try {
        return await fn();
      } finally {
        suspended--;
      }
    },
  };
}
