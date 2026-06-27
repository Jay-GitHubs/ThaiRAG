import { useEffect, useRef, useState } from 'react';
import { Alert, Spin } from 'antd';
import * as pdfjsLib from 'pdfjs-dist';
import { getDocumentOriginal } from '../api/conversations';

// Vite resolves this to a bundled worker asset.
pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).toString();

/** Renders a document's original PDF and scrolls to the cited page. */
export function PdfViewer({ docId, page }: { docId: string; page?: number }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    (async () => {
      try {
        const data = await getDocumentOriginal(docId);
        if (cancelled) return;
        const pdf = await pdfjsLib.getDocument({ data }).promise;
        const container = containerRef.current;
        if (!container || cancelled) return;
        container.innerHTML = '';

        for (let n = 1; n <= pdf.numPages; n++) {
          const pg = await pdf.getPage(n);
          if (cancelled) return;
          const viewport = pg.getViewport({ scale: 1.3 });
          const canvas = document.createElement('canvas');
          canvas.width = viewport.width;
          canvas.height = viewport.height;
          canvas.style.width = '100%';
          canvas.style.height = 'auto';
          canvas.style.marginBottom = '12px';
          canvas.style.border = '1px solid var(--line)';
          canvas.style.borderRadius = '6px';
          canvas.setAttribute('data-testid', 'pdf-page');
          canvas.setAttribute('data-page', String(n));
          container.appendChild(canvas);
          const ctx = canvas.getContext('2d');
          if (!ctx) continue;
          await pg.render({ canvasContext: ctx, viewport }).promise;
          if (page && n === page) canvas.scrollIntoView({ block: 'start' });
        }
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
  }, [docId, page]);

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
