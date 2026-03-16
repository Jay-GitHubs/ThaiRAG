import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

test.describe('Advanced Features — Context Compaction & Personal Memory', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    // Ensure pipeline is ON (advanced features are inside the pipeline card)
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }
  });

  // ── Context Compaction ────────────────────────────────────────────

  test('context compaction panel is visible with toggle and status tag', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    await expect(panel).toBeVisible();

    // Should have a switch and a tag (ON or OFF)
    await expect(panel.locator('.ant-switch')).toBeVisible();
    await expect(panel.locator('.ant-tag')).toBeVisible();
  });

  test('toggling context compaction shows parameter inputs', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    const toggle = panel.locator('.ant-switch');

    // Turn ON if not already
    const checked = await toggle.getAttribute('aria-checked');
    if (checked !== 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }

    // Expand the panel
    await panel.locator('.ant-collapse-header').click();
    await page.waitForTimeout(300);

    // Should show description
    await expect(page.getByText('Automatically compacts conversation history')).toBeVisible();

    // Should show parameter inputs
    await expect(page.getByText('Model Context Window')).toBeVisible();
    await expect(page.getByText('Compaction Threshold')).toBeVisible();
    await expect(page.getByText('Keep Recent Messages')).toBeVisible();
  });

  test('context compaction parameters are hidden when disabled', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    const toggle = panel.locator('.ant-switch');

    // Turn OFF
    const checked = await toggle.getAttribute('aria-checked');
    if (checked === 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }

    // Expand the panel
    await panel.locator('.ant-collapse-header').click();
    await page.waitForTimeout(300);

    // Description should still be visible
    await expect(page.getByText('Automatically compacts conversation history')).toBeVisible();

    // Parameter inputs should NOT be visible
    await expect(page.getByText('Model Context Window')).not.toBeVisible();
    await expect(page.getByText('Compaction Threshold')).not.toBeVisible();
    await expect(page.getByText('Keep Recent Messages')).not.toBeVisible();
  });

  test('context compaction tag shows ON/OFF status', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    const toggle = panel.locator('.ant-switch');

    // Turn ON
    const checked = await toggle.getAttribute('aria-checked');
    if (checked !== 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }

    // Tag should show "ON"
    await expect(panel.locator('.ant-tag').filter({ hasText: 'ON' })).toBeVisible();

    // Turn OFF
    await toggle.click();
    await page.waitForTimeout(300);

    // Tag should show "OFF"
    await expect(panel.locator('.ant-tag').filter({ hasText: 'OFF' })).toBeVisible();
  });

  // ── Personal Memory ───────────────────────────────────────────────

  test('personal memory panel is visible with toggle and status tag', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    await expect(panel).toBeVisible();

    await expect(panel.locator('.ant-switch')).toBeVisible();
    await expect(panel.locator('.ant-tag')).toBeVisible();
  });

  test('toggling personal memory shows parameter inputs', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    const toggle = panel.locator('.ant-switch');

    // Turn ON if not already
    const checked = await toggle.getAttribute('aria-checked');
    if (checked !== 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }

    // Expand the panel
    await panel.locator('.ant-collapse-header').click();
    await page.waitForTimeout(300);

    // Should show description
    await expect(page.getByText('Stores per-user memories in the vector database')).toBeVisible();

    // Should show parameter inputs
    await expect(page.getByText('Memories Per Query')).toBeVisible();
    await expect(page.getByText('Max Per User')).toBeVisible();
    await expect(page.getByText('Decay Factor')).toBeVisible();
    await expect(page.getByText('Min Relevance')).toBeVisible();
  });

  test('personal memory parameters are hidden when disabled', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    const toggle = panel.locator('.ant-switch');

    // Turn OFF
    const checked = await toggle.getAttribute('aria-checked');
    if (checked === 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }

    // Expand the panel
    await panel.locator('.ant-collapse-header').click();
    await page.waitForTimeout(300);

    // Description should still be visible
    await expect(page.getByText('Stores per-user memories in the vector database')).toBeVisible();

    // Parameter inputs should NOT be visible
    await expect(page.getByText('Memories Per Query')).not.toBeVisible();
    await expect(page.getByText('Max Per User')).not.toBeVisible();
    await expect(page.getByText('Decay Factor')).not.toBeVisible();
    await expect(page.getByText('Min Relevance')).not.toBeVisible();
  });

  test('personal memory tag shows ON/OFF status', async ({ page }) => {
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    const toggle = panel.locator('.ant-switch');

    // Turn ON
    const checked = await toggle.getAttribute('aria-checked');
    if (checked !== 'true') {
      await toggle.click();
      await page.waitForTimeout(300);
    }

    await expect(panel.locator('.ant-tag').filter({ hasText: 'ON' })).toBeVisible();

    // Turn OFF
    await toggle.click();
    await page.waitForTimeout(300);

    await expect(panel.locator('.ant-tag').filter({ hasText: 'OFF' })).toBeVisible();
  });

  // ── Persistence ───────────────────────────────────────────────────

  test('context compaction and personal memory settings persist after save and reload', async ({ page }) => {
    // Enable context compaction
    const ccPanel = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    const ccToggle = ccPanel.locator('.ant-switch');
    const ccChecked = await ccToggle.getAttribute('aria-checked');
    if (ccChecked !== 'true') {
      await ccToggle.click();
      await page.waitForTimeout(300);
    }

    // Enable personal memory
    const pmPanel = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    const pmToggle = pmPanel.locator('.ant-switch');
    const pmChecked = await pmToggle.getAttribute('aria-checked');
    if (pmChecked !== 'true') {
      await pmToggle.click();
      await page.waitForTimeout(300);
    }

    // Save
    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Response Pipeline' });
    await pipelineCard.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    // Reload
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    // Both should show ON tags (persisted)
    const ccPanelAfter = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    await expect(ccPanelAfter.locator('.ant-tag').filter({ hasText: 'ON' })).toBeVisible({ timeout: 5000 });

    const pmPanelAfter = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    await expect(pmPanelAfter.locator('.ant-tag').filter({ hasText: 'ON' })).toBeVisible({ timeout: 5000 });
  });

  test('disabling features persists after save and reload', async ({ page }) => {
    // Disable context compaction
    const ccPanel = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    const ccToggle = ccPanel.locator('.ant-switch');
    const ccChecked = await ccToggle.getAttribute('aria-checked');
    if (ccChecked === 'true') {
      await ccToggle.click();
      await page.waitForTimeout(300);
    }

    // Disable personal memory
    const pmPanel = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    const pmToggle = pmPanel.locator('.ant-switch');
    const pmChecked = await pmToggle.getAttribute('aria-checked');
    if (pmChecked === 'true') {
      await pmToggle.click();
      await page.waitForTimeout(300);
    }

    // Save
    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Response Pipeline' });
    await pipelineCard.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    // Reload
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    // Both should show OFF tags
    const ccPanelAfter = page.locator('.ant-collapse-item').filter({ hasText: 'Context Compaction' });
    await expect(ccPanelAfter.locator('.ant-tag').filter({ hasText: 'OFF' })).toBeVisible({ timeout: 5000 });

    const pmPanelAfter = page.locator('.ant-collapse-item').filter({ hasText: 'Personal Memory' });
    await expect(pmPanelAfter.locator('.ant-tag').filter({ hasText: 'OFF' })).toBeVisible({ timeout: 5000 });
  });
});
