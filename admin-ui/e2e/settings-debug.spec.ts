import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

test('debug auto_params round-trip', async ({ page }) => {
  await login(page);
  await navigateTo(page, 'Settings');

  // Go to Document Processing tab
  await page.getByRole('tab', { name: 'Document Processing' }).click();
  await page.waitForTimeout(1000);

  // Check the current state of AI Document Preprocessing switch
  const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
  const aiChecked = await aiSwitch.getAttribute('aria-checked');
  console.log('=== AI Preprocessing switch aria-checked:', aiChecked);

  // If AI is off, turn it on
  if (aiChecked !== 'true') {
    console.log('Turning AI preprocessing ON...');
    await aiSwitch.click();
    await page.waitForTimeout(500);
  }

  // Find the auto_params switch using data-testid
  const autoSwitch = page.getByTestId('auto-params-switch');
  await expect(autoSwitch).toBeVisible({ timeout: 5000 });

  const autoChecked = await autoSwitch.getAttribute('aria-checked');
  console.log('=== Auto params switch aria-checked:', autoChecked);

  // Read the label text next to the switch
  const labelText = await page.locator('text=AI adjusts per document').or(page.locator('text=Manual — fixed values')).first().textContent();
  console.log('=== Label text:', labelText);

  // Screenshot current state
  await page.screenshot({ path: 'e2e/screenshots/auto-params-1-initial.png' });

  // Force toggle to AUTO (true)
  if (autoChecked !== 'true') {
    console.log('Toggling auto switch ON...');
    await autoSwitch.click();
    await page.waitForTimeout(500);
  }

  const afterToggle = await autoSwitch.getAttribute('aria-checked');
  console.log('=== After toggle, auto switch aria-checked:', afterToggle);
  await page.screenshot({ path: 'e2e/screenshots/auto-params-2-after-toggle.png' });

  // Save
  console.log('Clicking Save...');
  await page.getByRole('button', { name: 'Save' }).first().click();
  await page.waitForTimeout(2000);

  // Check response - intercept network
  const afterSaveChecked = await autoSwitch.getAttribute('aria-checked');
  console.log('=== After save, auto switch aria-checked:', afterSaveChecked);
  await page.screenshot({ path: 'e2e/screenshots/auto-params-3-after-save.png' });

  // Reload page to verify persistence
  console.log('Reloading page...');
  await page.reload();
  await page.waitForTimeout(2000);
  await page.getByRole('tab', { name: 'Document Processing' }).click();
  await page.waitForTimeout(1500);

  await page.screenshot({ path: 'e2e/screenshots/auto-params-4-after-reload.png' });

  // Check if AI is still enabled after reload
  const aiSwitchReload = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
  const aiCheckedReload = await aiSwitchReload.getAttribute('aria-checked');
  console.log('=== After reload, AI switch aria-checked:', aiCheckedReload);

  // Find auto switch after reload
  const autoSwitchReload = page.getByTestId('auto-params-switch');
  // It might not be visible if AI preprocessing got turned off
  const autoVisible = await autoSwitchReload.isVisible();
  console.log('=== After reload, auto switch visible:', autoVisible);

  if (autoVisible) {
    const finalChecked = await autoSwitchReload.getAttribute('aria-checked');
    console.log('=== After reload, auto switch aria-checked:', finalChecked);

    const finalLabel = await page.locator('text=AI adjusts per document').or(page.locator('text=Manual — fixed values')).first().textContent();
    console.log('=== After reload, label:', finalLabel);

    // Verify it persisted as auto
    expect(finalChecked).toBe('true');
  } else {
    console.log('=== Auto switch not visible — AI preprocessing might have been turned off');
    // Turn AI on and check
    if (aiCheckedReload !== 'true') {
      await aiSwitchReload.click();
      await page.waitForTimeout(500);
    }
    const autoSwitchRetry = page.getByTestId('auto-params-switch');
    const retryChecked = await autoSwitchRetry.getAttribute('aria-checked');
    console.log('=== After turning AI on again, auto switch aria-checked:', retryChecked);
    expect(retryChecked).toBe('true');
  }
});

test('debug auto_params API round-trip', async ({ page }) => {
  await login(page);

  // Intercept the API calls to see what's being sent and received
  const apiCalls: { url: string; method: string; body?: string; response?: string }[] = [];

  page.on('request', (request) => {
    if (request.url().includes('/api/km/settings/document')) {
      apiCalls.push({
        url: request.url(),
        method: request.method(),
        body: request.postData() || undefined,
      });
    }
  });

  page.on('response', async (response) => {
    if (response.url().includes('/api/km/settings/document')) {
      try {
        const body = await response.text();
        const existing = apiCalls.find(c => c.url === response.url() && !c.response);
        if (existing) existing.response = body;
        else apiCalls.push({ url: response.url(), method: 'RESPONSE', response: body });
      } catch { /* ignore */ }
    }
  });

  await navigateTo(page, 'Settings');
  await page.getByRole('tab', { name: 'Document Processing' }).click();
  await page.waitForTimeout(2000);

  console.log('\n=== API calls during page load ===');
  for (const call of apiCalls) {
    console.log(`${call.method} ${call.url}`);
    if (call.body) console.log('  Request:', call.body);
    if (call.response) {
      try {
        const parsed = JSON.parse(call.response);
        console.log('  Response auto_params:', parsed.ai_preprocessing?.auto_params);
        console.log('  Response enabled:', parsed.ai_preprocessing?.enabled);
      } catch {
        console.log('  Response:', call.response.substring(0, 200));
      }
    }
  }

  // Now toggle and save
  const aiSwitch = page.locator('.ant-card-head').filter({ hasText: 'AI Document Preprocessing' }).locator('.ant-switch');
  const aiChecked = await aiSwitch.getAttribute('aria-checked');
  if (aiChecked !== 'true') {
    await aiSwitch.click();
    await page.waitForTimeout(500);
  }

  const autoSwitch = page.getByTestId('auto-params-switch');
  await expect(autoSwitch).toBeVisible({ timeout: 5000 });

  // Toggle to auto ON
  const autoChecked = await autoSwitch.getAttribute('aria-checked');
  if (autoChecked !== 'true') {
    await autoSwitch.click();
    await page.waitForTimeout(500);
  }

  // Clear api calls
  apiCalls.length = 0;

  // Save
  await page.getByRole('button', { name: 'Save' }).first().click();
  await page.waitForTimeout(2000);

  console.log('\n=== API calls during save ===');
  for (const call of apiCalls) {
    console.log(`${call.method} ${call.url}`);
    if (call.body) {
      try {
        const parsed = JSON.parse(call.body);
        console.log('  Request auto_params:', parsed.ai_preprocessing?.auto_params);
      } catch {
        console.log('  Request:', call.body.substring(0, 200));
      }
    }
    if (call.response) {
      try {
        const parsed = JSON.parse(call.response);
        console.log('  Response auto_params:', parsed.ai_preprocessing?.auto_params);
      } catch {
        console.log('  Response:', call.response.substring(0, 200));
      }
    }
  }
});
