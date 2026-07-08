import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

// Headed e2e for the factory reset. Runs a GLOBAL CONTENT reset (keeps users +
// structure, so the authenticated session stays valid) through the admin UI:
// Settings → Vector Database → Danger Zone → Factory Reset.
// DESTRUCTIVE: wipes every workspace's documents, chunks, vectors and the BM25
// index on the target stack. It must never run as part of a default full-suite
// pass — opt in explicitly with E2E_FACTORY_RESET=1 against a disposable stack.
test.skip(
  process.env.E2E_FACTORY_RESET !== '1',
  'destructive against live KM content; set E2E_FACTORY_RESET=1 to opt in',
);

test('factory reset (global content) runs from the admin UI', async ({ page }) => {
  test.setTimeout(120_000);
  await login(page);
  await navigateTo(page, 'Settings');
  await page.getByRole('tab', { name: 'Vector Database' }).click();

  // Open the Danger Zone collapse panel, where the reset controls live.
  await page.getByText('Danger Zone', { exact: true }).click();
  await expect(page.getByTestId('factory-reset')).toBeVisible();

  // Scope defaults to global, mode to content. Submit is gated on typing RESET.
  await expect(page.getByTestId('reset-submit')).toBeDisabled();
  await page.getByTestId('reset-confirm').fill('RESET');
  await expect(page.getByTestId('reset-submit')).toBeEnabled();

  await page.getByTestId('reset-submit').click();
  await page.getByRole('button', { name: 'Yes, reset' }).click();

  await expect(page.getByText(/Factory reset complete/i)).toBeVisible({ timeout: 30_000 });
});
