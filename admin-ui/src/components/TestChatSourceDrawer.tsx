import { Drawer, Spin, Tag, Typography } from 'antd';
import { useQuery } from '@tanstack/react-query';
import { getDocumentContent } from '../api/documents';
import type { Citation, RetrievedChunk } from '../api/types';
import { RichTextView } from './RichTextView';

/**
 * In-app source viewer for a cited chunk. Fetches the document's converted text
 * and highlights the retrieved chunk within it (full-document context, like the
 * end-user chat UI). Falls back to the chunk text alone if the document can't be
 * loaded. Read-only; opens from a citation chip on a test-chat answer.
 */
export function TestChatSourceDrawer({
  open,
  onClose,
  citation,
  chunk,
  workspaceId,
}: {
  open: boolean;
  onClose: () => void;
  citation: Citation | null;
  chunk?: RetrievedChunk;
  workspaceId?: string;
}) {
  const docId = citation?.doc_id;
  const { data, isLoading } = useQuery({
    queryKey: ['doc-content', workspaceId, docId],
    queryFn: () => getDocumentContent(workspaceId!, docId!),
    enabled: open && !!workspaceId && !!docId,
    staleTime: 5 * 60_000,
  });

  // The cited chunk is the evidence; highlight it inside the full converted text.
  const snippet = chunk?.content || citation?.claim || '';
  const fullText = data?.converted_text || chunk?.content || '';
  const title = citation?.doc_title || chunk?.doc_title || citation?.doc_id || 'Source';

  const meta = [
    chunk?.page_numbers?.length ? `p.${chunk.page_numbers.join(', ')}` : null,
    chunk?.section_title || null,
    chunk ? `score ${chunk.score.toFixed(3)}` : null,
  ].filter(Boolean) as string[];

  return (
    <Drawer
      open={open}
      onClose={onClose}
      width={Math.min(720, typeof window !== 'undefined' ? window.innerWidth : 720)}
      title={
        <span data-testid="source-drawer-title" style={{ wordBreak: 'break-word' }}>
          {title}
        </span>
      }
    >
      {meta.length > 0 && (
        <div
          data-testid="source-provenance"
          style={{
            background: 'var(--celadon-tint)',
            border: '1px solid var(--chip-border)',
            borderRadius: 8,
            padding: '8px 12px',
            marginBottom: 14,
            fontSize: 13,
            color: 'var(--celadon-deep)',
          }}
        >
          Cited from {meta.join(' · ')}
        </div>
      )}

      {isLoading ? (
        <div style={{ textAlign: 'center', padding: 40 }}>
          <Spin />
        </div>
      ) : fullText ? (
        <RichTextView content={fullText} snippet={snippet} />
      ) : (
        <Typography.Paragraph type="secondary">
          No readable content for this document.
        </Typography.Paragraph>
      )}

      {!data?.converted_text && chunk && (
        <Tag style={{ marginTop: 12 }}>Showing the retrieved chunk (full document unavailable)</Tag>
      )}
    </Drawer>
  );
}
