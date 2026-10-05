import { test, expect } from '@playwright/test';

// The discrete-event-systems org's LIVE public web surface.
//
// The reliable default is the org's GitHub Pages site (see org_map / README:
// homepage https://discrete-event-systems.github.io) — a static Astro site with
// distinct /simulations/ and /games/ galleries. There is no publicly reachable
// app domain yet (des-web.rs, the dynamic MASH companion, is not deployed to a
// public URL), so the Pages site + the org's GitHub page are the surfaces we
// can meaningfully assert against.
const PAGES_ORIGIN = 'https://discrete-event-systems.github.io';
const ORG_URL = 'https://github.com/discrete-event-systems';

// HARD assertion on the reliably-live surface: the org's GitHub Pages home must
// load with an ok status and identify itself as the DES site. If this fails,
// the public site is genuinely down.
test('org Pages home loads and is the DES site', async ({ page }) => {
  const resp = await page.goto(`${PAGES_ORIGIN}/`, { waitUntil: 'domcontentloaded' });
  expect(resp, 'no response from Pages home').toBeTruthy();
  expect(resp!.status(), `unexpected status from ${PAGES_ORIGIN}/`).toBeLessThan(400);
  await expect(page).toHaveTitle(/Discrete Event Systems/i);
});

// The two galleries live on the same Pages deploy. Hard-assert the HTTP
// response and the (very stable) document title; the <h1> wording may be
// refreshed over time, so log + soft-check it rather than failing the job.
const GALLERIES = [
  { path: '/simulations/', heading: 'Watch state become history.' },
  { path: '/games/', heading: 'Decisions are part of the model.' },
] as const;

for (const { path, heading } of GALLERIES) {
  test(`gallery ${path} loads`, async ({ page }) => {
    const resp = await page.goto(`${PAGES_ORIGIN}${path}`, { waitUntil: 'domcontentloaded' });
    expect(resp, `no response from ${path}`).toBeTruthy();
    expect(resp!.status(), `unexpected status from ${path}`).toBeLessThan(400);
    await expect(page).toHaveTitle(/Discrete Event Systems/i);

    const h1 = (await page.locator('h1').first().textContent())?.trim() ?? '';
    if (h1 !== heading) {
      console.log(`note: ${path} <h1> is now ${JSON.stringify(h1)} (was ${JSON.stringify(heading)})`);
    }
    expect.soft(h1.length, `${path} has no <h1> text`).toBeGreaterThan(0);
  });
}

// Minimal backstop liveness: the org's GitHub page itself is reachable. Uses an
// API request (no browser navigation needed) so it stays cheap and robust.
test('org GitHub page is reachable', async ({ request }) => {
  const resp = await request.get(ORG_URL, { timeout: 20_000 });
  expect(resp.status(), `unexpected status from ${ORG_URL}`).toBeLessThan(400);
  expect(await resp.text()).toContain('discrete-event-systems');
});
