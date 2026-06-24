import { test, expect, type Page } from '@playwright/test';
import { login, navigateTo } from './helpers';

// The pre-ingest review+override feature exposes two Document-Processing
// settings: an "Always preview before ingest" gate and a tunable image-coverage
// routing threshold. These specs prove they render, save the right request
// shape, and persist.

const DOC_PUT = '/api/km/settings/document';

function capturePut(page: Page, urlFragment: string) {
  const bodies: string[] = [];
  page.on('request', (req) => {
    if (req.url().includes(urlFragment) && req.method() === 'PUT') {
      const data = req.postData();
      if (data) bodies.push(data);
    }
  });
  return {
    last: () => (bodies.length ? JSON.parse(bodies[bodies.length - 1]) : null),
    count: () => bodies.length,
  };
}

test.describe('Document handling settings (always-preview + thresholds)', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
  });

  test('the new controls render', async ({ page }) => {
    await expect(page.getByTestId('always-preview-switch')).toBeVisible();
    await expect(page.getByText('Always preview before ingest')).toBeVisible();
    await expect(page.getByText('Image-coverage threshold')).toBeVisible();
  });

  // The image-coverage input is the only step=0.05 / max=1 spinbutton on the tab.
  const covInput = (page: Page) =>
    page.locator(
      'xpath=//span[contains(text(),"Image-coverage threshold")]/ancestor::div[contains(@class,"ant-space-vertical")][1]//input',
    );

  test('always-preview + image-coverage save and persist', async ({ page }) => {
    const sw = page.getByTestId('always-preview-switch');
    await sw.scrollIntoViewIfNeeded();

    // Turn the gate ON.
    if ((await sw.getAttribute('aria-checked')) !== 'true') {
      await sw.click();
      await page.waitForTimeout(200);
    }
    await expect(sw).toHaveAttribute('aria-checked', 'true');

    // Change the image-coverage threshold input (commit with Tab).
    await covInput(page).fill('0.4');
    await page.keyboard.press('Tab');

    // Save via the Pipeline Settings card.
    const card = page.locator('.ant-card').filter({ hasText: 'Pipeline Settings' });
    const put = capturePut(page, DOC_PUT);
    await card.getByRole('button', { name: 'Save' }).first().click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    expect(put.count()).toBeGreaterThan(0);
    const body = put.last();
    expect(body.always_preview).toBe(true);
    expect(body.pdf_page_as_image_threshold).toBe(0.4);

    // Persistence: reload → the gate toggle is still ON.
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
    await expect(page.getByTestId('always-preview-switch')).toHaveAttribute('aria-checked', 'true');

    // Restore defaults (don't leave the gate on for other flows).
    await page.getByTestId('always-preview-switch').click();
    await page.waitForTimeout(200);
    await covInput(page).fill('0.5');
    await page.keyboard.press('Tab');
    const put2 = capturePut(page, DOC_PUT);
    await page
      .locator('.ant-card')
      .filter({ hasText: 'Pipeline Settings' })
      .getByRole('button', { name: 'Save' })
      .first()
      .click();
    await page.waitForTimeout(1500);
    expect(put2.last().always_preview).toBe(false);
    expect(put2.last().pdf_page_as_image_threshold).toBe(0.5);
  });
});
