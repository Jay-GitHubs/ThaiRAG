import { useEffect, useState } from 'react';
import { Alert, Drawer, Grid, Segmented, Spin } from 'antd';
import { getDocumentSource } from '../api/conversations';
import type { Citation, DocumentSource } from '../api/types';
import { PdfViewer } from './PdfViewer';
import { RichTextView } from './RichTextView';

export function SourceDrawer({
  citation,
  onClose,
}: {
  citation: Citation | null;
  onClose: () => void;
}) {
  const [doc, setDoc] = useState<DocumentSource | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Original-PDF view vs converted-text view. Defaults to the PDF for PDFs.
  const [view, setView] = useState<'pdf' | 'text'>('text');
  const screens = Grid.useBreakpoint();

  useEffect(() => {
    if (!citation) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDoc(null);
    getDocumentSource(citation.doc_id)
      .then((d) => {
        if (cancelled) return;
        setDoc(d);
        setView(d.mime_type === 'application/pdf' ? 'pdf' : 'text');
      })
      .catch(() => !cancelled && setError('Could not load this source.'))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [citation]);

  const isPdf = doc?.mime_type === 'application/pdf';

  const prov = [
    citation?.section ? `Section ${citation.section}` : null,
    citation?.page ? `p.${citation.page}` : null,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <Drawer
      open={!!citation}
      onClose={onClose}
      title={doc?.title ?? citation?.title ?? 'Source'}
      width={screens.md ? 600 : '100%'}
      styles={{ body: { padding: '16px 20px' } }}
    >
      {loading && (
        <div style={{ textAlign: 'center', padding: '40px 0' }}>
          <Spin />
        </div>
      )}
      {error && <Alert type="error" showIcon message={error} />}
      {doc && (
        <>
          {prov && (
            <div
              data-testid="source-provenance"
              style={{
                background: 'var(--celadon-tint)',
                border: '1px solid #cfe3dd',
                borderRadius: 8,
                padding: '8px 12px',
                marginBottom: 14,
                fontSize: 13,
                color: 'var(--celadon-deep)',
              }}
            >
              Cited from {prov}
            </div>
          )}
          {isPdf && (
            <Segmented
              value={view}
              onChange={(v) => setView(v as 'pdf' | 'text')}
              options={[
                { label: 'Document', value: 'pdf' },
                { label: 'Text', value: 'text' },
              ]}
              style={{ marginBottom: 14 }}
            />
          )}

          {view === 'pdf' && isPdf ? (
            <PdfViewer docId={doc.doc_id} page={citation?.page} snippet={citation?.snippet} />
          ) : (
            <RichTextView content={doc.content} snippet={citation?.snippet} />
          )}
          {citation?.url && (
            <div style={{ marginTop: 18, fontSize: 12.5 }}>
              <a href={citation.url} target="_blank" rel="noreferrer" style={{ color: 'var(--celadon-deep)' }}>
                Open full document in a new tab ↗
              </a>
            </div>
          )}
        </>
      )}
    </Drawer>
  );
}
