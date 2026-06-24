import fs from 'fs';
import path from 'path';
import { test, expect } from '@playwright/test';
import { login, navigateTo, TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

// Playwright runs specs as ESM (no __dirname); resolve fixtures from cwd
// (the admin-ui dir when `npm run test:e2e` runs), one level up to the repo root.
const FIXTURE_PDF = path.resolve(process.cwd(), '../tests/fixtures/borderless_table.pdf');

// The Pipeline provenance popover surfaces an "Extraction" line so an operator
// can see, per document, WHICH engine ran — e.g. whether PaddleOCR transcribed
// pages. This drives that rendering end-to-end: ingest a tiny PDF in High-Quality
// mode (OCRs every page via the deterministic tier), then hover the Pipeline tag
// and assert the Extraction line shows the OCR page count + provider.
//
// Needs the live stack with the PaddleOCR sidecar wired (compose profile "ocr").

test.describe('Extraction line in the Pipeline popover', () => {
  const suffix = Date.now();
  const orgName = `ExOrg-${suffix}`;
  const deptName = `ExDept-${suffix}`;
  const wsName = `ExWS-${suffix}`;

  let token: string;
  let orgId: string;
  let wsId: string;

  test.beforeAll(async ({ request }) => {
    token = (await (await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    })).json()).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: orgName }, headers })).json()).id;
    const deptId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: deptName }, headers })).json()).id;
    wsId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, { data: { name: wsName }, headers })).json()).id;

    // Ingest a tiny PDF in High-Quality mode → every page OCR'd by the
    // deterministic tier (fast: 1 page, small enrichment). Wait until READY and
    // confirm provenance carries the OCR engine before driving the UI.
    test.skip(!fs.existsSync(FIXTURE_PDF), 'tiny PDF fixture not present');
    const up = await request.post(`${API_BASE}/api/km/workspaces/${wsId}/documents/upload`, {
      headers,
      multipart: {
        file: { name: 'extract.pdf', mimeType: 'application/pdf', buffer: fs.readFileSync(FIXTURE_PDF) },
        title: 'extract-doc',
        handling_mode: 'high_quality',
      },
    });
    expect(up.ok()).toBeTruthy();
    const docId = (await up.json()).doc_id;
    let ex: { ocr_pages_used?: number; ocr_provider?: string } | undefined;
    for (let i = 0; i < 40; i++) {
      const d = await (await request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}`, { headers })).json();
      if (d.status === 'ready' || d.status === 'failed') {
        ex = d.processing_provenance?.extraction;
        break;
      }
      await new Promise((r) => setTimeout(r, 3000));
    }
    // Precondition: the OCR tier must actually have run, else the UI assertion
    // below would be vacuous. (If the sidecar isn't wired, skip rather than fail.)
    test.skip(!ex || (ex.ocr_pages_used ?? 0) < 1, 'OCR tier did not run (sidecar not wired)');
    expect(ex!.ocr_provider).toBe('paddleocr-sidecar');
  });

  test.afterAll(async ({ request }) => {
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers: { Authorization: `Bearer ${token}` } });
  });

  test('hovering the Pipeline tag shows the Extraction line with OCR + provider', async ({ page }) => {
    await login(page);
    await navigateTo(page, 'Documents');
    await expect(page.getByRole('heading', { name: 'Documents' })).toBeVisible();

    await page.locator('.ant-select', { hasText: /Select Organization/i }).click();
    await page.getByTitle(orgName).click();
    await page.locator('.ant-select', { hasText: /Select Department/i }).click();
    await page.getByTitle(deptName).click();
    await page.locator('.ant-select', { hasText: /Select Workspace/i }).click();
    await page.getByTitle(wsName).click();

    // The doc row + its Pipeline tag (the path label) must be visible.
    await expect(page.getByText('extract-doc')).toBeVisible({ timeout: 10000 });
    const pipelineTag = page.locator('.ant-table-tbody .ant-tag', { hasText: /smart-PDF/i }).first();
    await expect(pipelineTag).toBeVisible();

    // Hover opens the "Processing details" popover containing the Extraction line.
    await pipelineTag.hover();
    const popover = page.locator('.ant-popover', { hasText: 'Processing details' });
    await expect(popover).toBeVisible({ timeout: 5000 });

    // The Extraction line: page count, the OCR tag with its count, and the
    // PaddleOCR provider name — the whole point of the feature.
    await expect(popover.getByText('Extraction', { exact: true })).toBeVisible();
    await expect(popover.getByText(/1 pages/)).toBeVisible();
    await expect(popover.getByText(/OCR 1/)).toBeVisible();
    await expect(popover.getByText(/paddleocr-sidecar/)).toBeVisible();
  });
});
