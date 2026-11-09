#!/usr/bin/env node
// Width audit for the collaboration pages: desktop and phone.
//
// Not part of the build or CI: it needs Playwright, which is not a dependency of
// this repository. Install it somewhere and point PLAYWRIGHT at it, e.g.
//
//   mkdir -p /tmp/pw && cd /tmp/pw && npm init -y && npm i playwright
//   cd ironforge && \
//     IRONFORGE_URL=http://127.0.0.1:5173 IRONFORGE_USER=me IRONFORGE_PASSWORD=… \
//     IRONFORGE_OWNER=me IRONFORGE_REPO=demo IRONFORGE_TASK=<task id> \
//     PLAYWRIGHT=/tmp/pw/node_modules/playwright node scripts/ui-width-audit.mjs
//
// It logs in through the API (the token is an HttpOnly cookie, so the form is
// not needed), opens every collaboration page at 1440 and 375 CSS pixels,
// reports whether the DOCUMENT scrolls horizontally, lists the elements wider
// than the viewport (a table inside an `overflow-x: auto` wrapper is expected
// and counted separately), and writes a screenshot per page and width.
//
// Exit code 1 if any page scrolls horizontally at 375.

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const url = process.env.IRONFORGE_URL || 'http://127.0.0.1:5173';
const owner = process.env.IRONFORGE_OWNER || '';
const repo = process.env.IRONFORGE_REPO || '';
const task = process.env.IRONFORGE_TASK || '';
const user = process.env.IRONFORGE_USER || '';
const password = process.env.IRONFORGE_PASSWORD || '';
const out = process.env.OUT_DIR || '/tmp/ironforge-width-audit';
// A bare specifier ("playwright") or a path to an installed copy.
const playwrightSpec = process.env.PLAYWRIGHT || 'playwright';
const playwrightPath =
	fs.existsSync(playwrightSpec) && fs.statSync(playwrightSpec).isDirectory()
		? path.join(playwrightSpec, 'index.js')
		: playwrightSpec;

if (!owner || !repo || !user || !password) {
	console.error('IRONFORGE_OWNER, IRONFORGE_REPO, IRONFORGE_USER and IRONFORGE_PASSWORD are required');
	process.exit(2);
}

const loaded = await import(
	playwrightPath.startsWith('/') ? pathToFileURL(playwrightPath).href : playwrightPath
);
// A CJS copy (index.js) arrives under `default`.
const { chromium } = loaded.chromium ? loaded : loaded.default || {};

const pages = [
	['overview', `/${owner}/${repo}/collaboration`],
	['task', task ? `/${owner}/${repo}/collaboration/tasks/${task}` : null],
	['health', `/${owner}/${repo}/collaboration/health`],
	['groups', `/${owner}/${repo}/collaboration/groups`],
	['rules', `/${owner}/${repo}/collaboration/rules`],
	['search', `/${owner}/${repo}/collaboration/search`],
	['settings', `/${owner}/${repo}/collaboration/settings`],
	['templates', `/${owner}/${repo}/collaboration/templates`],
].filter(([, href]) => href);

fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch(process.env.CHROME_CHANNEL ? { channel: process.env.CHROME_CHANNEL } : {});
const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });

const login = await context.request.post(`${url}/api/v1/users/login`, {
	data: { username: user, password }
});
const cookie = /ironforge_token=([^;]+)/.exec(login.headers()['set-cookie'] || '');
if (!login.ok() || !cookie) {
	console.error(`login failed: ${login.status()}`);
	process.exit(2);
}
const host = new URL(url).hostname;
await context.addCookies([
	{ name: 'ironforge_token', value: cookie[1], domain: host, path: '/', httpOnly: true }
]);

const page = await context.newPage();
let failed = 0;
for (const [name, href] of pages) {
	for (const width of [1440, 375]) {
		await page.setViewportSize({ width, height: 900 });
		await page.goto(url + href, { waitUntil: 'networkidle' });
		await page.waitForTimeout(500);
		const result = await page.evaluate(() => {
			const vw = window.innerWidth;
			const offenders = [];
			for (const el of document.querySelectorAll('body *')) {
				const rect = el.getBoundingClientRect();
				if (!rect.width || !rect.height) continue;
				if (getComputedStyle(el).position === 'fixed') continue;
				if (rect.right <= vw + 1 && rect.left >= -1) continue;
				// Inside a horizontal scroller is by design, not overflow of the page.
				let scroller = el.parentElement;
				let inScroller = false;
				while (scroller) {
					const style = getComputedStyle(scroller);
					if (style.overflowX === 'auto' || style.overflowX === 'scroll') {
						inScroller = true;
						break;
					}
					scroller = scroller.parentElement;
				}
				offenders.push({
					tag: el.tagName.toLowerCase(),
					width: Math.round(rect.width),
					inScroller
				});
			}
			return {
				vw,
				pageScrollWidth: document.documentElement.scrollWidth,
				overflow: document.documentElement.scrollWidth > vw + 1,
				outsideScroller: offenders.filter((o) => !o.inScroller).slice(0, 5),
				insideScroller: offenders.filter((o) => o.inScroller).length
			};
		});
		await page.screenshot({ path: path.join(out, `${name}-${width}.png`) });
		if (result.overflow) failed += 1;
		console.log(
			`${name.padEnd(10)} ${String(width).padEnd(5)} ` +
				`${result.overflow ? `OVERFLOW +${result.pageScrollWidth - result.vw}px` : 'ok'}` +
				(result.insideScroller ? `  (${result.insideScroller} element(s) inside a horizontal scroller)` : '') +
				(result.outsideScroller.length ? `  outside: ${JSON.stringify(result.outsideScroller)}` : '')
		);
	}
}
await browser.close();
console.log(`report and screenshots in ${out}`);
process.exit(failed ? 1 : 0);
