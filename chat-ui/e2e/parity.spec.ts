import { test, expect } from '@playwright/test';
import { login, TEST_EMAIL, TEST_PASSWORD } from './helpers';

// Final OWUI-parity gate: exercises the full first-party feature set against the
// live stack. Streaming tests depend on a working chat model on the deployment.
const COMPOSER = 'Ask anything about your documents…';

/** Wait for a streamed answer to finish (the composer re-enables). */
async function waitForAnswer(page: import('@playwright/test').Page) {
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 120_000 });
}

test('login page offers SSO providers (G1)', async ({ page }) => {
  await page.goto('/login');
  await expect(page.getByRole('button', { name: /Continue with/ }).first()).toBeVisible({
    timeout: 15_000,
  });
});

test('scoped chat streams an answer with source citations (scope + citations)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  // Pin the conversation to the KMs workspace (which holds the Micro Pay manual).
  await page.locator('.ant-select-selector').first().click();
  await page
    .locator('.ant-select-item-option')
    .filter({ hasText: /^KMs$/ })
    .click();

  await page
    .getByPlaceholder(COMPOSER)
    .fill('วิธีเข้าสู่ระบบ (log-in) ของแอป Micro Pay ทำอย่างไร');
  await page.getByRole('button', { name: 'Send' }).click();

  await waitForAnswer(page);
  await expect(page.getByTestId('msg-assistant').last()).toBeVisible();
  await expect(page.getByText('Sources', { exact: true })).toBeVisible({ timeout: 10_000 });
});

test('regenerate replaces the answer without duplicating the turn (G2)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('hello there');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  await page.getByRole('button', { name: 'Regenerate' }).click();
  await waitForAnswer(page);

  // Exactly one user + one assistant — regenerate replaced, didn't append.
  await expect(page.getByTestId('msg-user')).toHaveCount(1);
  await expect(page.getByTestId('msg-assistant')).toHaveCount(1);
});

test('thumbs feedback persists across reload (G5)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('rate this answer');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  // Thumbs-up the answer; it survives a reload.
  await page.getByTestId('fb-up').last().click();
  await page.reload();
  await expect(page.getByTestId('msg-assistant').last()).toBeVisible({ timeout: 15_000 });
  // The filled (active) like icon renders with the celadon-deep accent colour.
  await expect(page.getByLabel('Remove positive feedback').last()).toBeVisible({ timeout: 10_000 });
});

test('stop halts a streaming answer (G2)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page
    .getByPlaceholder(COMPOSER)
    .fill('Explain this system in detail, step by step, with examples.');
  await page.getByRole('button', { name: 'Send' }).click();

  // Stop appears while streaming; clicking it ends the stream (composer re-enables).
  await page.getByRole('button', { name: 'Stop' }).click({ timeout: 30_000 });
  await expect(page.getByPlaceholder(COMPOSER)).toBeEnabled({ timeout: 30_000 });
});

test('file attachment is accepted and answered (file upload)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();

  await page.setInputFiles('input[type=file]', {
    name: 'note.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('The capital of the example country is Exampleville.'),
  });
  await expect(page.getByText('note.txt')).toBeVisible();

  await page.getByPlaceholder(COMPOSER).fill('What does the attached note say?');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);
  await expect(page.getByTestId('msg-user').filter({ hasText: 'What does the attached note say?' })).toBeVisible();
});

test('history persists across reload (persistence)', async ({ page }) => {
  await login(page);
  await page.getByRole('button', { name: 'New chat' }).click();
  await page.getByPlaceholder(COMPOSER).fill('remember this turn');
  await page.getByRole('button', { name: 'Send' }).click();
  await waitForAnswer(page);

  await page.reload();
  await expect(page.getByTestId('msg-user').filter({ hasText: 'remember this turn' })).toBeVisible({
    timeout: 15_000,
  });
});

test.describe('mobile', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('drawer navigation works on a phone viewport (G3)', async ({ page }) => {
    // Can't use login() here: on mobile the sidebar (and its New chat button) is
    // behind the drawer, so assert the composer instead as the post-login signal.
    await page.goto('/login');
    await page.getByLabel('Email').fill(TEST_EMAIL);
    await page.getByLabel('Password').fill(TEST_PASSWORD);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByPlaceholder(COMPOSER)).toBeVisible({ timeout: 15_000 });

    // The hamburger opens the conversation drawer.
    await page.getByRole('button', { name: 'Menu' }).click();
    await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible();
  });
});
