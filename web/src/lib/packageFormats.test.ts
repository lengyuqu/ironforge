import { describe, it, expect } from 'vitest';
import {
  PACKAGE_FORMATS,
  PACKAGE_FORMAT_LABELS,
  NATIVE_PACKAGE_FORMATS,
  packageFormatLabel,
  packageFormatUsesGenericFallback,
  packageFormatSupportLabel,
  packageFormatOptionLabel,
} from './packageFormats';

// These tests pin the format taxonomy the upload form relies on: which
// formats get the native adapter, which fall back to the generic registry,
// and how each is labelled in the picker.
describe('packageFormats', () => {
  it('every known format has a display label', () => {
    for (const f of PACKAGE_FORMATS) {
      // 'npm' legitimately maps to itself; the invariant is that a label
      // entry exists (otherwise the function degrades to the raw key).
      expect(PACKAGE_FORMAT_LABELS[f]).toBeTruthy();
    }
  });

  it('unknown formats fall back to the raw string', () => {
    expect(packageFormatLabel('totally-unknown')).toBe('totally-unknown');
  });

  it('native formats are exactly the native set', () => {
    for (const f of NATIVE_PACKAGE_FORMATS) {
      expect(packageFormatUsesGenericFallback(f)).toBe(false);
    }
    const nativeSet: readonly string[] = NATIVE_PACKAGE_FORMATS;
    const fallbackFormats = PACKAGE_FORMATS.filter((f) => !nativeSet.includes(f));
    expect(fallbackFormats.length).toBeGreaterThan(0);
    for (const f of fallbackFormats) {
      expect(packageFormatUsesGenericFallback(f)).toBe(true);
    }
  });

  it('support and option labels reflect the adapter tier', () => {
    expect(packageFormatSupportLabel('cargo')).toBe('Native adapter');
    expect(packageFormatSupportLabel('go')).toBe('Generic fallback');
    expect(packageFormatOptionLabel('npm')).toBe('npm');
    expect(packageFormatOptionLabel('rpm')).toBe('RPM (Generic fallback)');
  });
});
