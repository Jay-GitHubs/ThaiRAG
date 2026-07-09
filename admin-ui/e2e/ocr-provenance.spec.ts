import fs from 'fs';
import path from 'path';
import { test, expect } from '@playwright/test';
import { TEST_EMAIL, TEST_PASSWORD, API_BASE } from './helpers';

// Specs run as ESM (no __dirname); resolve fixtures from cwd (the admin-ui dir).
// Fixture choice is load-bearing: the 47-page tfac_gazette took 19 MINUTES to
// ingest on the all-gateway stack (measured 2026-07-09) and blew every budget.
// The 2-page scanned gazette exercises the same provenance path (scanned →
// OCR/vision engines recorded) in a few minutes.
const FIXTURE_PDF = path.resolve(
  process.cwd(),
  '../tests/fixtures/thai-real/scanned_gazette_2486.pdf',
);

// After processing, a document's provenance records WHICH extraction engines ran
// (deterministic OCR vs vision LLM) — so an operator can see, per document and
// without reading container logs, whether e.g. PaddleOCR transcribed any pages.
// This drives the Pipeline popover's "Extraction" line in the admin UI.
//
// Requires the live stack with the PaddleOCR sidecar (compose profile "ocr") and
// a vision model configured, so High-Quality mode actually OCRs pages. Skips
// cleanly when the deterministic OCR tier isn't wired (provenance then shows 0).

test.describe('OCR provenance visibility', () => {
  const suffix = Date.now();
  let token: string;
  let orgId: string;
  let wsId: string;

  test.beforeAll(async ({ request }) => {
    token = (await (await request.post(`${API_BASE}/api/auth/login`, {
      data: { email: TEST_EMAIL, password: TEST_PASSWORD },
    })).json()).token;
    const headers = { Authorization: `Bearer ${token}` };
    orgId = (await (await request.post(`${API_BASE}/api/km/orgs`, { data: { name: `OcrOrg-${suffix}` }, headers })).json()).id;
    const deptId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts`, { data: { name: 'd' }, headers })).json()).id;
    wsId = (await (await request.post(`${API_BASE}/api/km/orgs/${orgId}/depts/${deptId}/workspaces`, { data: { name: 'w' }, headers })).json()).id;
  });

  test.afterAll(async ({ request }) => {
    await request.delete(`${API_BASE}/api/km/orgs/${orgId}`, { headers: { Authorization: `Bearer ${token}` } });
  });

  test('High-Quality reprocess records OCR/vision engine usage in provenance', async ({ request }) => {
    // Two full processing passes (ingest + HQ reprocess) with AI
    // preprocessing over the gateway — minutes each, not seconds.
    test.setTimeout(1_500_000);
    const headers = { Authorization: `Bearer ${token}` };
    test.skip(!fs.existsSync(FIXTURE_PDF), 'Thai fixture PDF not present');

    // Ingest the Thai PDF and wait for ready.
    const buffer = fs.readFileSync(FIXTURE_PDF);
    const up = await request.post(`${API_BASE}/api/km/workspaces/${wsId}/documents/upload`, {
      headers,
      multipart: {
        file: { name: 'gazette.pdf', mimeType: 'application/pdf', buffer },
        title: 'ocr-prov-gazette',
      },
    });
    const docId = (await up.json()).doc_id;
    const waitReady = async () => {
      for (let i = 0; i < 120; i++) {
        const d = await (await request.get(`${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}`, { headers })).json();
        if (d.status === 'ready' || d.status === 'failed') return d;
        await new Promise((r) => setTimeout(r, 5000));
      }
      throw new Error('document did not finish processing');
    };
    await waitReady();

    // Reprocess in High-Quality mode → every page OCR'd (deterministic tier
    // preferred when configured).
    await request.post(`${API_BASE}/api/km/workspaces/${wsId}/documents/${docId}/reprocess`, {
      headers,
      data: { handling_mode: 'high_quality' },
    });
    const doc = await waitReady();

    const ex = doc.processing_provenance?.extraction;
    expect(ex, 'provenance must carry an extraction block after smart-PDF').toBeTruthy();
    expect(ex.total_pages).toBeGreaterThan(0);

    // The whole point: post-hoc, the engine that ran is recorded — not just
    // predicted. With the OCR tier wired, High-Quality OCRs every page and names
    // the provider; otherwise it falls to vision. Either way an engine is named.
    const ocr = ex.ocr_pages_used ?? 0;
    const vision = ex.vision_pages_used ?? 0;
    expect(ocr + vision, 'High-Quality must record OCR or vision pages').toBeGreaterThan(0);
    if (ocr > 0) {
      expect(ex.ocr_provider, 'OCR pages must name the provider').toBeTruthy();
    } else {
      expect(ex.vision_model, 'vision pages must name the model').toBeTruthy();
    }
  });
});
