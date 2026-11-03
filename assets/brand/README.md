# IronForge brand assets

The IronForge mark is a git branch rising from an anvil: the forge (铁匠铺)
where commits are shaped.

| File | Use |
|------|-----|
| `ironforge-mark.svg` | Bare mark, transparent background; colours follow `prefers-color-scheme` |
| `ironforge-icon.svg` | Mark on the iron tile; source for the favicon and app icons |
| `ironforge-logo.svg` | Mark + wordmark lockup (README header) |

Colours: iron `#2b2622`, steel `#eceff1`, ember `#f5a524` (`#d97706` on light
backgrounds).

Inside the SPA the mark is rendered by `web/src/lib/components/BrandMark.svelte`
(anvil in `currentColor`, branch in `--brand-ember`); keep its geometry in sync
with `ironforge-mark.svg`.

The raster icons in `web/static/` (`favicon.ico`, `apple-touch-icon.png`,
`icon-192.png`, `icon-512.png`, `icon-maskable-512.png`) and
`web/static/favicon.svg` are generated from `ironforge-icon.svg`:

```bash
scripts/gen-brand-icons.sh
```
