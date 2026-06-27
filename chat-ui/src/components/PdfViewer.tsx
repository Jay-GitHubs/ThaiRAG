import { useEffect, useRef, useState } from 'react';
import { Alert, Spin } from 'antd';
import * as pdfjsLib from 'pdfjs-dist';
import { getDocumentOriginal } from '../api/conversations';

// Vite resolves this to a bundled worker asset.
pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).toString();

// Keep only alphanumerics (Latin + Thai + digits); drop whitespace and
// punctuation. This lets a converted-markdown snippet (e.g. a `| North | 100 |`
// table) match the PDF text layer's plain "North 100".
const norm = (s: string) => s.replace(/[^0-9a-z฀-๿]/gi, '').toLowerCase();

/**
 * Renders a document's original PDF, scrolls to the cited page, and — for
 * born-digital PDFs with a text layer — highlights the cited passage by
 * matching the snippet against the page text and overlaying boxes on the canvas.
 * Scanned PDFs have no text layer, so highlighting is skipped (page jump only).
 */
export function PdfViewer({
  docId,
  page,
  snippet,
}: {
  docId: string;
  page?: number;
  snippet?: string;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    const needle = snippet ? norm(snippet).slice(0, 80) : '';

    (async () => {
      try {
        const data = await getDocumentOriginal(docId);
        if (cancelled) return;
        const pdf = await pdfjsLib.getDocument({ data }).promise;
        const container = containerRef.current;
        if (!container || cancelled) return;
        container.innerHTML = '';
        let firstHighlight: HTMLElement | null = null;

        for (let n = 1; n <= pdf.numPages; n++) {
          const pg = await pdf.getPage(n);
          if (cancelled) return;
          const viewport = pg.getViewport({ scale: 1.3 });

          const wrap = document.createElement('div');
          wrap.style.position = 'relative';
          wrap.style.marginBottom = '12px';
          wrap.setAttribute('data-page', String(n));

          const canvas = document.createElement('canvas');
          canvas.width = viewport.width;
          canvas.height = viewport.height;
          canvas.style.width = '100%';
          canvas.style.height = 'auto';
          canvas.style.display = 'block';
          canvas.style.border = '1px solid var(--line)';
          canvas.style.borderRadius = '6px';
          canvas.setAttribute('data-testid', 'pdf-page');
          wrap.appendChild(canvas);
          container.appendChild(wrap);

          const ctx = canvas.getContext('2d');
          if (!ctx) continue;
          await pg.render({ canvasContext: ctx, viewport }).promise;
          if (cancelled) return;

          // Highlight the cited snippet on this page (born-digital PDFs only).
          if (needle.length >= 10) {
            const tc = await pg.getTextContent();
            const items = tc.items.filter((it) => 'str' in it) as unknown as Array<{
              str: string;
              transform: number[];
              width: number;
            }>;
            // Whitespace-stripped page text + a map back to the item index.
            let stripped = '';
            const itemOf: number[] = [];
            items.forEach((it, idx) => {
              for (const ch of norm(it.str)) {
                stripped += ch;
                itemOf.push(idx);
              }
            });
            let pos = stripped.indexOf(needle);
            if (pos < 0 && needle.length > 40) pos = stripped.indexOf(needle.slice(0, 40));
            if (pos >= 0) {
              const endPos = Math.min(stripped.length - 1, pos + needle.length - 1);
              const start = itemOf[pos];
              const end = itemOf[endPos];
              for (let i = start; i <= end; i++) {
                const it = items[i];
                const tx = pdfjsLib.Util.transform(viewport.transform, it.transform);
                const fontH = Math.hypot(tx[1], tx[3]);
                const left = tx[4];
                const top = tx[5] - fontH;
                const w = it.width * viewport.scale;
                const box = document.createElement('div');
                box.setAttribute('data-testid', 'pdf-highlight');
                box.style.position = 'absolute';
                box.style.left = `${(left / viewport.width) * 100}%`;
                box.style.top = `${(top / viewport.height) * 100}%`;
                box.style.width = `${(w / viewport.width) * 100}%`;
                box.style.height = `${(fontH / viewport.height) * 100}%`;
                box.style.background = 'var(--mark-bg)';
                box.style.borderRadius = '2px';
                box.style.pointerEvents = 'none';
                wrap.appendChild(box);
                if (!firstHighlight) firstHighlight = box;
              }
            }
          }

          if (page && n === page && !firstHighlight) canvas.scrollIntoView({ block: 'start' });
        }

        if (firstHighlight) firstHighlight.scrollIntoView({ block: 'center' });
        if (!cancelled) setLoading(false);
      } catch {
        if (!cancelled) {
          setError('Could not render the original PDF.');
          setLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [docId, page, snippet]);

  return (
    <div>
      {loading && (
        <div style={{ textAlign: 'center', padding: '40px 0' }}>
          <Spin />
        </div>
      )}
      {error && <Alert type="error" showIcon message={error} />}
      <div ref={containerRef} data-testid="pdf-viewer" />
    </div>
  );
}
