import { describe, it, expect, beforeEach, vi } from 'vitest';
import { toast, type ToastItem } from '$lib/components/toast.svelte';

describe('toast store', () => {
  beforeEach(() => {
    toast.clear();
    vi.useFakeTimers();
  });

  it('starts with an empty items list', () => {
    expect(toast.items).toEqual([]);
  });

  it('toast.show pushes one item with default info type', () => {
    const item = toast.show({ message: 'hello' });
    expect(item.type).toBe('info');
    expect(item.message).toBe('hello');
    expect(toast.items).toHaveLength(1);
    expect(toast.items[0]!.id).toBe(item.id);
  });

  it('toast.success / error / warning use matching types', () => {
    toast.success('ok');
    toast.warning('warn');
    toast.error('fail');
    toast.info('note');
    expect(toast.items.map((t: ToastItem) => [t.type, t.message])).toEqual([
      ['success', 'ok'],
      ['warning', 'warn'],
      ['error', 'fail'],
      ['info', 'note'],
    ]);
  });

  it('auto-dismisses items after their duration elapses', () => {
    toast.success('bye', 1500);
    expect(toast.items).toHaveLength(1);
    vi.advanceTimersByTime(1499);
    expect(toast.items).toHaveLength(1);
    vi.advanceTimersByTime(2);
    expect(toast.items).toHaveLength(0);
  });

  it('dismiss(id) removes a specific toast without touching others', () => {
    const a = toast.info('a');
    const b = toast.info('b');
    const c = toast.info('c');
    toast.dismiss(b.id);
    expect(toast.items.map((t: ToastItem) => t.id)).toEqual([a.id, c.id]);
  });

  it('clear() empties the list immediately', () => {
    toast.info('x');
    toast.info('y');
    toast.clear();
    expect(toast.items).toEqual([]);
  });
});
