import { test, expect, type Page } from '@playwright/test';
import { login, navigateTo } from './helpers';

// Smart-PDF is enforced for every PDF: the adaptive engine always runs (text
// layer for clean pages, vision OCR for image/scanned/corrupted-text-layer
// pages). The "High quality (OCR every page)" knob is now always visible and
// governs the always-on PDF vision path. These specs prove the UI reflects the
// enforcement and that High Quality round-trips through the document settings.

const DOC_PUT = '/api/km/settings/document';

/** Capture the body of the next matching PUT, returning getters resolved after it fires. */
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

test.describe('Smart-PDF enforcement (Document Processing tab)', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
  });

  test('UI states that PDFs are always processed with adaptive Smart-PDF', async ({ page }) => {
    await expect(
      page.getByText('PDFs are always processed with adaptive Smart-PDF', { exact: false }),
    ).toBeVisible();
    // The High Quality knob is visible without first enabling any opt-in toggle.
    await expect(page.getByTestId('pdf-high-quality-switch')).toBeVisible();
  });

  test('High Quality toggles, saves pdf_high_quality, and persists', async ({ page }) => {
    const hq = page.getByTestId('pdf-high-quality-switch');
    await hq.scrollIntoViewIfNeeded();

    // Turn High Quality ON.
    if ((await hq.getAttribute('aria-checked')) !== 'true') {
      await hq.click();
      await page.waitForTimeout(200);
    }
    await expect(hq).toHaveAttribute('aria-checked', 'true');

    // Save via the Pipeline Settings card → PUT carries pdf_high_quality: true.
    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Pipeline Settings' });
    const putOn = capturePut(page, DOC_PUT);
    await pipelineCard.getByRole('button', { name: 'Save' }).first().click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });
    expect(putOn.count()).toBeGreaterThan(0);
    expect(putOn.last().pdf_high_quality).toBe(true);

    // Persistence: reload → switch is still ON.
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Document Processing' }).click();
    await page.waitForTimeout(1000);
    await expect(page.getByTestId('pdf-high-quality-switch')).toHaveAttribute(
      'aria-checked',
      'true',
    );

    // Restore default: turn it OFF and save → pdf_high_quality: false.
    const hq2 = page.getByTestId('pdf-high-quality-switch');
    await hq2.scrollIntoViewIfNeeded();
    await hq2.click();
    await page.waitForTimeout(200);
    await expect(hq2).toHaveAttribute('aria-checked', 'false');

    const putOff = capturePut(page, DOC_PUT);
    await page
      .locator('.ant-card')
      .filter({ hasText: 'Pipeline Settings' })
      .getByRole('button', { name: 'Save' })
      .first()
      .click();
    await page.waitForTimeout(1500);
    expect(putOff.count()).toBeGreaterThan(0);
    expect(putOff.last().pdf_high_quality).toBe(false);
  });
});
