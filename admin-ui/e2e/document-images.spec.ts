import { test, expect, type Page } from '@playwright/test';
import { login, navigateTo } from './helpers';

// Deterministic UI test for the ChunksModal "Extracted Images" gallery.
// The image-extraction pipeline depends on slow vision-LLM ingestion, so instead
// of ingesting a real image-bearing doc we mock the KM endpoints and the image
// blob bytes. This proves the gallery renders (and the authed blob→object-URL
// path works) without any ingestion flakiness.

// 1×1 transparent PNG.
const PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

async function mockKm(page: Page) {
  const json = (body: unknown) => ({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });

  // Org → Dept → Workspace cascade.
  await page.route(/\/api\/km\/orgs($|\?)/, (route) =>
    route.fulfill(json({ data: [{ id: 'org-1', name: 'MockOrg' }], total: 1 })),
  );
  await page.route(/\/orgs\/[^/]+\/depts($|\?)/, (route) =>
    route.fulfill(json({ data: [{ id: 'dept-1', name: 'MockDept' }], total: 1 })),
  );
  await page.route(/\/depts\/[^/]+\/workspaces($|\?)/, (route) =>
    route.fulfill(json({ data: [{ id: 'ws-1', name: 'MockWS' }], total: 1 })),
  );

  // Jobs panel above the table — keep it empty/quiet.
  await page.route(/\/workspaces\/[^/]+\/jobs($|\?)/, (route) =>
    route.fulfill(json({ data: [], total: 0 })),
  );
  await page.route(/\/workspaces\/[^/]+\/jobs\/stream/, (route) =>
    route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' }),
  );

  // One ready document.
  await page.route(/\/workspaces\/[^/]+\/documents($|\?)/, (route) =>
    route.fulfill(
      json({
        data: [
          {
            id: 'doc-1',
            workspace_id: 'ws-1',
            title: 'Mock Image Doc',
            mime_type: 'application/pdf',
            size_bytes: 1234,
            status: 'ready',
            chunk_count: 1,
            created_at: new Date().toISOString(),
            updated_at: new Date().toISOString(),
          },
        ],
        total: 1,
      }),
    ),
  );

  // Modal contents.
  await page.route(/\/documents\/[^/]+\/chunks($|\?)/, (route) =>
    route.fulfill(
      json({
        doc_id: 'doc-1',
        chunks: [{ chunk_id: 'chunk-aaaaaaaa', text: 'hello world', page: null, index: 0 }],
        total: 1,
      }),
    ),
  );
  await page.route(/\/documents\/[^/]+\/images($|\?)/, (route) =>
    route.fulfill(
      json([
        {
          image_id: 'img-1',
          mime: 'image/png',
          width: 1,
          height: 1,
          page_num: 2,
          source: 'pdf_page_render',
        },
      ]),
    ),
  );
  // The actual image bytes (authed blob fetch → object URL).
  await page.route(/\/documents\/[^/]+\/images\/[^/]+($|\?)/, (route) =>
    route.fulfill({
      status: 200,
      contentType: 'image/png',
      body: Buffer.from(PNG_B64, 'base64'),
    }),
  );
}

async function pickScope(page: Page, testId: string, label: string) {
  await page.locator(`[data-tour="${testId}"] .ant-select-selector`).click();
  await page.locator(`.ant-select-item-option[title="${label}"]`).click();
}

test.describe('ChunksModal extracted-images gallery', () => {
  test('renders extracted images inline in the chunks modal', async ({ page }) => {
    await login(page);
    await mockKm(page);
    await navigateTo(page, 'Documents');

    // Drive the org/dept/workspace cascade to reveal the document table.
    await pickScope(page, 'doc-org-select', 'MockOrg');
    await pickScope(page, 'doc-dept-select', 'MockDept');
    await pickScope(page, 'doc-ws-select', 'MockWS');

    await expect(page.getByText('Mock Image Doc')).toBeVisible({ timeout: 10_000 });

    // Open the chunks modal (the BlockOutlined "View chunks" button).
    await page.locator('button:has(.anticon-block)').first().click();

    const modal = page.locator('.ant-modal-content');
    await expect(modal.getByText('Chunks: Mock Image Doc', { exact: false })).toBeVisible();

    // Gallery section + rendered image.
    await expect(modal.getByText('Extracted Images (1)')).toBeVisible();
    await expect(modal.getByText('Page 2 · 1×1 · pdf_page_render')).toBeVisible();

    const img = modal.locator('.ant-image img').first();
    await expect(img).toBeVisible({ timeout: 10_000 });
    await expect(img).toHaveAttribute('src', /^blob:/);

    // Chunk text still renders alongside the gallery.
    await expect(modal.getByText('hello world')).toBeVisible();
  });
});
