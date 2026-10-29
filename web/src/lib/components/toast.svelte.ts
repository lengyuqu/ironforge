// Toast notification manager — Svelte 5 runes.
//
// Usage from any component:
//   import { toast } from '$lib/components/toast';
//   toast.show({ message: 'Saved!', type: 'success' });
//   toast.success('Saved!');
//   toast.error('Failed: ' + err.message);
//
// Render `<ToastContainer />` once in the root layout — all toasts stack there.

export type ToastType = 'info' | 'success' | 'warning' | 'error';

export interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
  /** Auto-dismiss after this many ms. 0 = no auto-dismiss. */
  duration: number;
}

const DEFAULT_DURATIONS: Record<ToastType, number> = {
  info: 4000,
  success: 3000,
  warning: 5000,
  error: 8000,
};

/** More than this many toasts at once drops the oldest. */
const MAX_VISIBLE = 5;

let nextId = 1;
const items = $state<ToastItem[]>([]);
const timers = new Map<number, { handle: ReturnType<typeof setTimeout>; started: number; remaining: number }>();

function arm(id: number, ms: number) {
  const handle = setTimeout(() => dismiss(id), ms);
  timers.set(id, { handle, started: Date.now(), remaining: ms });
}

function push(message: string, type: ToastType = 'info', duration?: number): ToastItem {
  const id = nextId++;
  const item: ToastItem = {
    id,
    message,
    type,
    duration: duration ?? DEFAULT_DURATIONS[type],
  };
  items.push(item);
  if (item.duration > 0) arm(id, item.duration);
  while (items.length > MAX_VISIBLE) dismiss(items[0].id);
  return item;
}

function dismiss(id: number) {
  const timer = timers.get(id);
  if (timer) clearTimeout(timer.handle);
  timers.delete(id);
  const idx = items.findIndex((t) => t.id === id);
  if (idx >= 0) items.splice(idx, 1);
}

export const toast = {
  get items() {
    return items;
  },
  show(opts: { message: string; type?: ToastType; duration?: number }) {
    return push(opts.message, opts.type ?? 'info', opts.duration);
  },
  info(message: string, duration?: number) {
    return push(message, 'info', duration);
  },
  success(message: string, duration?: number) {
    return push(message, 'success', duration);
  },
  warning(message: string, duration?: number) {
    return push(message, 'warning', duration);
  },
  error(message: string, duration?: number) {
    return push(message, 'error', duration);
  },
  dismiss,
  /** Stop the auto-dismiss countdown (e.g. while hovered or focused). */
  pause(id: number) {
    const timer = timers.get(id);
    if (!timer) return;
    clearTimeout(timer.handle);
    timer.remaining = Math.max(1000, timer.remaining - (Date.now() - timer.started));
  },
  /** Restart the countdown with whatever time was left when paused. */
  resume(id: number) {
    const timer = timers.get(id);
    if (timer) arm(id, timer.remaining);
  },
  clear() {
    for (const timer of timers.values()) clearTimeout(timer.handle);
    timers.clear();
    items.length = 0;
  },
};
