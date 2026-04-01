import { type Page, expect } from '@playwright/test';

export const TEST_EMAIL = 'playwright@test.com';
export const TEST_PASSWORD = 'Test1234!';
export const API_BASE = 'http://localhost:8080';

/** Suppress guided tours and quick start from auto-starting during tests. */
export async function suppressTours(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('thairag-tour-state', '{}');
    localStorage.setItem('thairag-quickstart-dismissed', 'true');
  });
}

/** Login via the UI and wait for dashboard to load. */
export async function login(page: Page) {
  await suppressTours(page);
  await page.goto('/login');
  await page.getByPlaceholder('Email').fill(TEST_EMAIL);
  await page.getByPlaceholder('Password').fill(TEST_PASSWORD);
  await page.getByRole('button', { name: 'Sign In' }).click();
  await page.waitForURL('/', { timeout: 10_000 });
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
}

/**
 * Navigate to a page via the sidebar menu (client-side navigation).
 * This avoids full page reload which would lose the in-memory auth token.
 * Handles grouped sub-menus by expanding parent groups if needed.
 */
export async function navigateTo(page: Page, menuLabel: string) {
  const menu = page.getByRole('menu');
  const target = menu.getByText(menuLabel, { exact: true });

  // If the target is already visible (top-level or group already open), click it
  if (await target.isVisible().catch(() => false)) {
    await target.click();
    return;
  }

  // Otherwise, expand each collapsed sub-menu group until we find it
  const subMenus = menu.locator('.ant-menu-submenu-title');
  const count = await subMenus.count();
  for (let i = 0; i < count; i++) {
    const submenu = subMenus.nth(i);
    // Check if this group is already open
    const parent = submenu.locator('..');
    const isOpen = await parent.evaluate((el) =>
      el.closest('.ant-menu-submenu')?.classList.contains('ant-menu-submenu-open'),
    );
    if (!isOpen) {
      await submenu.click();
      // Check if our target is now visible
      if (await target.isVisible().catch(() => false)) {
        await target.click();
        return;
      }
      // Collapse it back if not the right group
      await submenu.click();
    } else {
      // Already open — check if target is inside
      if (await target.isVisible().catch(() => false)) {
        await target.click();
        return;
      }
    }
  }

  // Fallback: try clicking directly (may throw if not visible)
  await target.click();
}
