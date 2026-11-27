// Renders SVG files to transparent PNGs with headless Chromium (Playwright).
// Chromium draws the icon's filters and gradients the way player.html does.
//
// Usage: node render.mjs <in.svg> <out.png> <px> [<in.svg> <out.png> <px> ...]
// Each SVG must have a square viewBox and width/height attributes; it is
// drawn at <px> x <px>.
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

const require = createRequire(import.meta.url);
let chromium;
try { ({ chromium } = require('playwright')); }
catch { ({ chromium } = require('/opt/node22/lib/node_modules/playwright')); }

const args = process.argv.slice(2);
if (args.length === 0 || args.length % 3 !== 0) {
  console.error('usage: node render.mjs <in.svg> <out.png> <px> ...');
  process.exit(2);
}
const browser = await chromium.launch();
try {
  for (let i = 0; i < args.length; i += 3) {
    const [src, out, pxs] = args.slice(i, i + 3);
    const px = Number(pxs);
    const svg = fs.readFileSync(src, 'utf8');
    const w = Number((svg.match(/<svg[^>]*\swidth="([\d.]+)"/) || [])[1] || 512);
    const scale = px / w;
    // Chromium caps the viewport; scale with deviceScaleFactor instead.
    const page = await browser.newPage({ viewport: { width: w, height: w }, deviceScaleFactor: scale });
    await page.goto(pathToFileURL(path.resolve(src)).href);
    await page.screenshot({ path: out, omitBackground: true, clip: { x: 0, y: 0, width: w, height: w } });
    await page.close();
  }
} finally {
  await browser.close();
}
