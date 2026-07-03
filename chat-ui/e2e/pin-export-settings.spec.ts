import { test, expect } from '@playwright/test';
import { login } from './helpers';

/**
 * Batch-C features: conversation pinning, Markdown export, and the account
 * settings modal (self-service password change). Live-stack gated; uses the
 * shared playwright test account's existing conversations.
 */

test('pin moves a conversation into the Pinned group and persists', async ({ page }) => {
  await login(page);

  const firstRow = page.getByTestId('conversation-row').first();
  await expect(firstRow).toBeVisible();
  const title = (await firstRow.innerText()).trim().split('\n')[0];

  // Pin via the hover action.
  await firstRow.hover();
  await firstRow.getByTestId('pin-toggle').click();
  await expect(page.getByText('Pinned', { exact: true })).toBeVisible();

  // Survives a reload (persisted server-side), then unpin restores recency.
  await page.reload();
  await expect(page.getByText('Pinned', { exact: true })).toBeVisible();
  const pinnedRow = page.getByTestId('conversation-row').first();
  await expect(pinnedRow).toContainText(title.slice(0, 20));
  await pinnedRow.hover();
  await pinnedRow.getByTestId('pin-toggle').click();
  await expect(page.getByText('Pinned', { exact: true })).toHaveCount(0);
});

test('export downloads the conversation as Markdown', async ({ page }) => {
  await login(page);

  const firstRow = page.getByTestId('conversation-row').first();
  await expect(firstRow).toBeVisible();
  await firstRow.hover();

  const downloadPromise = page.waitForEvent('download');
  await firstRow.getByTestId('export-conversation').click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toMatch(/\.md$/);
});

test('settings modal shows the account and rejects a wrong current password', async ({
  page,
}) => {
  await login(page);

  await page.getByTestId('settings-button').click();
  // Scope to the modal — the sidebar footer also shows the email.
  await expect(page.locator('.ant-modal').getByText('playwright@test.com')).toBeVisible();

  await page.getByTestId('current-password').fill('Wrong123');
  await page.getByTestId('new-password').fill('NewPass123');
  await page.getByTestId('confirm-password').fill('NewPass123');
  await page.getByTestId('save-password').click();

  // Backend rejects; the modal stays open and the account is unchanged (other
  // specs keep logging in with the original password).
  await expect(page.getByTestId('save-password')).toBeVisible();
  await expect(page.locator('.ant-message-notice').first()).toBeVisible({
    timeout: 5_000,
  });
});
