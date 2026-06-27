import { useEffect, useRef, useState } from 'react';
import { Alert, Drawer, Grid, Spin } from 'antd';
import { getDocumentSource } from '../api/conversations';
import type { Citation, DocumentSource } from '../api/types';

/**
 * Locate the cited passage inside the full document text so we can highlight it.
 * We have no stored character offsets, so this is a best-effort text match on the
 * snippet (falls back to shorter prefixes). Returns null if it can't be found.
 */
function buildHighlight(content: string, snippet?: string) {
  const cand = snippet?.trim();
  if (!cand) return null;
  let needle = cand;
  let idx = content.indexOf(needle);
  if (idx < 0 && cand.length > 60) {
    needle = cand.slice(0, 60);
    idx = content.indexOf(needle);
  }
  if (idx < 0 && cand.length > 30) {
    needle = cand.slice(0, 30);
    idx = content.indexOf(needle);
  }
  if (idx < 0) return null;
  const end = Math.min(content.length, idx + Math.max(needle.length, cand.length));
  return { before: content.slice(0, idx), match: content.slice(idx, end), after: content.slice(end) };
}

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
  const markRef = useRef<HTMLElement>(null);
  const screens = Grid.useBreakpoint();

  useEffect(() => {
    if (!citation) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDoc(null);
    getDocumentSource(citation.doc_id)
      .then((d) => !cancelled && setDoc(d))
      .catch(() => !cancelled && setError('Could not load this source.'))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [citation]);

  // Scroll the highlighted passage into view once the document renders.
  useEffect(() => {
    if (doc && markRef.current) {
      markRef.current.scrollIntoView({ block: 'center', behavior: 'smooth' });
    }
  }, [doc]);

  const hl = doc ? buildHighlight(doc.content, citation?.snippet) : null;
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
          {!hl && citation?.snippet && (
            <Alert
              type="info"
              showIcon
              style={{ marginBottom: 14 }}
              message="Couldn't pinpoint the exact passage — showing the full document."
            />
          )}
          <div
            data-testid="source-content"
            style={{
              whiteSpace: 'pre-wrap',
              wordWrap: 'break-word',
              fontSize: 14,
              lineHeight: 1.7,
              color: 'var(--ink)',
            }}
          >
            {hl ? (
              <>
                {hl.before}
                <mark
                  ref={markRef}
                  data-testid="source-highlight"
                  style={{ background: '#fff3bf', padding: '1px 2px', borderRadius: 3 }}
                >
                  {hl.match}
                </mark>
                {hl.after}
              </>
            ) : (
              doc.content
            )}
          </div>
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
