// Renders a list of HTML jobs to PNG with Chromium (Playwright).
// Usage: node render.js jobs.json    (each job: {out, w, h, scale, html})
// Run from build.py; it supersamples by `scale` and build.py downsamples.
const { chromium } = require('playwright');
const fs = require('fs');
(async () => {
  const jobs = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
  const browser = await chromium.launch();
  const pages = {};
  for (const j of jobs) {
    const key = j.scale;
    if (!pages[key]) pages[key] = await (await browser.newContext({ deviceScaleFactor: j.scale })).newPage();
    const page = pages[key];
    await page.setViewportSize({ width: j.w, height: j.h });
    await page.setContent(j.html, { waitUntil: 'load' });
    await page.evaluate(() => Promise.all([...document.images].map(i => i.decode().catch(() => {}))));
    await page.screenshot({ path: j.out, omitBackground: true, clip: { x: 0, y: 0, width: j.w, height: j.h } });
  }
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
