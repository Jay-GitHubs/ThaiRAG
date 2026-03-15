import { test, expect } from '@playwright/test';
import { login, navigateTo } from './helpers';

test.describe('Chat & Response Pipeline Tab', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Settings');
    // The first tab is "Chat & Response Pipeline"
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);
  });

  test('shows pipeline switch and agent panels when enabled', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    await expect(pipelineSwitch).toBeVisible();

    // Ensure pipeline is ON
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Pipeline flow diagram should be visible
    await expect(page.getByText('Pipeline flow')).toBeVisible();

    // Parameters should be visible
    await expect(page.getByText('Max Context Tokens')).toBeVisible();
    await expect(page.getByText('Agent Max Tokens')).toBeVisible();

    // Agent LLM Configuration section
    await expect(page.getByText('Agent LLM Configuration')).toBeVisible();

    // Agent panels should be visible in Collapse
    await expect(page.getByText('Query Analyzer')).toBeVisible();
    await expect(page.getByText('Pipeline Orchestrator')).toBeVisible();
    await expect(page.getByText('Query Rewriter')).toBeVisible();
    await expect(page.getByText('Context Curator')).toBeVisible();
    await expect(page.getByText('Response Generator')).toBeVisible();
    await expect(page.getByRole('button', { name: /Quality Guard/ })).toBeVisible();
    await expect(page.getByRole('button', { name: /Language Adapter/ })).toBeVisible();
  });

  test('shows feature list when pipeline is disabled', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');

    // Ensure pipeline is OFF
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked === 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Should show the feature list
    await expect(page.getByText('Enable to activate the intelligent multi-agent pipeline')).toBeVisible();
    await expect(page.getByText('Smart routing')).toBeVisible();
    await expect(page.getByText('Query expansion')).toBeVisible();

    // Agent panels should NOT be visible
    await expect(page.getByText('Pipeline flow')).not.toBeVisible();
  });

  test('LLM mode selector switches between chat/shared/per-agent', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Default should be one of the modes — switch to "Shared"
    await page.getByText('Shared', { exact: true }).click();
    await page.waitForTimeout(300);

    // Should show "Dedicated LLM shared by all pipeline agents"
    await expect(page.getByText('Dedicated LLM shared by all')).toBeVisible();

    // Switch to "Per-Agent"
    await page.getByText('Per-Agent', { exact: true }).click();
    await page.waitForTimeout(300);

    // Should show per-agent description
    await expect(page.getByText('Each agent can use a different LLM')).toBeVisible();

    // Switch to "Use Chat LLM"
    await page.getByText('Use Chat LLM', { exact: true }).click();
    await page.waitForTimeout(300);

    // Should show chat LLM description
    await expect(page.getByText('All agents use the main Chat LLM')).toBeVisible();
  });

  test('per-agent mode persists after save and reload', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Switch to Per-Agent mode
    await page.getByText('Per-Agent', { exact: true }).click();
    await page.waitForTimeout(300);

    // Save
    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Response Pipeline' });
    await pipelineCard.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });

    // Reload to verify persistence
    await page.reload();
    await page.waitForTimeout(1500);
    await page.getByRole('tab', { name: 'Chat & Response Pipeline' }).click();
    await page.waitForTimeout(1000);

    // Per-Agent description should be visible (mode persisted)
    await expect(page.getByText('Each agent can use a different LLM')).toBeVisible({ timeout: 5000 });

    // Restore to chat mode for other tests
    await page.getByText('Use Chat LLM', { exact: true }).click();
    await page.waitForTimeout(300);
    await pipelineCard.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
  });

  test('expanding agent panel shows description and hints', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Expand Response Generator panel
    await page.getByText('Response Generator').click();
    await page.waitForTimeout(300);

    // Should show description and hints
    await expect(page.getByText('Generates the final answer using curated context')).toBeVisible();
    await expect(page.getByText('Runs:')).toBeVisible();
    await expect(page.getByText('If disabled:')).toBeVisible();
    await expect(page.getByText('LLM tip:')).toBeVisible();
    await expect(page.getByText('Heavy workload')).toBeVisible();
  });

  test('pipeline orchestrator panel shows max LLM calls setting', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Enable orchestrator if needed
    const orchPanel = page.locator('.ant-collapse-item').filter({ hasText: 'Pipeline Orchestrator' });
    const orchSwitch = orchPanel.locator('.ant-switch');
    const orchChecked = await orchSwitch.getAttribute('aria-checked');
    if (orchChecked !== 'true') {
      await orchSwitch.click();
      await page.waitForTimeout(300);
    }

    // Expand orchestrator panel
    await orchPanel.locator('.ant-collapse-header').click();
    await page.waitForTimeout(300);

    // Should show max LLM calls setting
    await expect(page.getByText('Max LLM Calls')).toBeVisible();
  });

  test('quality guard panel shows threshold and retries settings', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Enable quality guard if needed
    const qgPanel = page.locator('.ant-collapse-item').filter({ hasText: 'Quality Guard' });
    const qgSwitch = qgPanel.locator('.ant-switch');
    const qgChecked = await qgSwitch.getAttribute('aria-checked');
    if (qgChecked !== 'true') {
      await qgSwitch.click();
      await page.waitForTimeout(300);
    }

    // Expand quality guard panel
    await qgPanel.locator('.ant-collapse-header').click();
    await page.waitForTimeout(300);

    // Should show threshold and retries
    await expect(page.getByText('Max Retries')).toBeVisible();
    await expect(page.getByText('Threshold', { exact: true })).toBeVisible();
  });

  test('streaming defense info is shown', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Streaming defense info Alert
    await expect(page.getByText('3-layer hallucination defense')).toBeVisible();
    await expect(page.getByText('Pre-stream:')).toBeVisible();
    await expect(page.getByText('Post-stream:')).toBeVisible();
  });

  test('save pipeline settings succeeds', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Save
    const pipelineCard = page.locator('.ant-card').filter({ hasText: 'Response Pipeline' });
    await pipelineCard.getByRole('button', { name: 'Save' }).click();
    await page.waitForTimeout(1500);
    await expect(page.getByText('saved')).toBeVisible({ timeout: 5000 });
  });

  test('per-agent LLM form appears inside agent panel', async ({ page }) => {
    const pipelineSwitch = page.getByTestId('chat-pipeline-switch');
    const checked = await pipelineSwitch.getAttribute('aria-checked');
    if (checked !== 'true') {
      await pipelineSwitch.click();
      await page.waitForTimeout(300);
    }

    // Switch to Per-Agent mode
    await page.getByText('Per-Agent', { exact: true }).click();
    await page.waitForTimeout(300);

    // Expand Response Generator panel
    const panel = page.locator('.ant-collapse-item').filter({ hasText: 'Response Generator' });
    await panel.locator('.ant-collapse-header').click();
    await page.waitForTimeout(300);

    // Should show "Agent LLM Override" with form
    await expect(page.getByText('Agent LLM Override').first()).toBeVisible();

    // Should show LLM kind selector and model input
    await expect(panel.locator('.ant-select').first()).toBeVisible();

    // Restore to chat mode
    await page.getByText('Use Chat LLM', { exact: true }).click();
    await page.waitForTimeout(300);
  });
});
