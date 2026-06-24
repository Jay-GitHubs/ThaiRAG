import { useState, useEffect } from 'react';
import { Modal, Button, Spin, Alert, message } from 'antd';
import { useReprocessDocument } from '../../hooks/useDocuments';
import { DocumentPreviewPanel } from './DocumentPreviewPanel';
import { HandlingControls } from './HandlingControls';
import { previewDocumentById } from '../../api/documents';
import type { Document, DocumentPreview, DocumentHandling } from '../../api/types';

interface Props {
  workspaceId: string;
  doc: Document;
  open: boolean;
  onClose: () => void;
}

/** Reprocess a document with an explicit, reviewed handling decision — the same
 *  preview + override levers as first upload, applied to an already-stored doc.
 *  Without this, reprocess silently re-runs in Auto mode and ignores any choice. */
export function ReprocessModal({ workspaceId, doc, open, onClose }: Props) {
  const [preview, setPreview] = useState<DocumentPreview | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [previewError, setPreviewError] = useState(false);
  const [handlingMode, setHandlingMode] = useState<DocumentHandling['handling_mode']>('auto');
  const [covThreshold, setCovThreshold] = useState<number | null>(null);
  const [minChars, setMinChars] = useState<number | null>(null);
  const reprocess = useReprocessDocument();

  // Auto-run the dry-run analysis when the modal opens so the admin always sees
  // what the pipeline WOULD do before committing the re-run.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setPreview(null);
    setPreviewError(false);
    setHandlingMode('auto');
    setCovThreshold(null);
    setMinChars(null);
    setPreviewing(true);
    previewDocumentById(workspaceId, doc.id)
      .then((p) => {
        if (!cancelled) setPreview(p);
      })
      .catch(() => {
        if (!cancelled) setPreviewError(true);
      })
      .finally(() => {
        if (!cancelled) setPreviewing(false);
      });
    // Closing the modal mid-request must not write state into an unmounted tree.
    return () => {
      cancelled = true;
    };
  }, [open, workspaceId, doc.id]);

  async function handleReprocess() {
    try {
      await reprocess.mutateAsync({
        wsId: workspaceId,
        docId: doc.id,
        handling: {
          handling_mode: handlingMode,
          image_coverage_threshold: covThreshold ?? undefined,
          min_chars_per_page: minChars ?? undefined,
        },
      });
      message.success('Reprocessing started');
      onClose();
    } catch {
      message.error('Failed to reprocess document');
    }
  }

  return (
    <Modal
      title={`Reprocess — ${doc.title}`}
      open={open}
      onCancel={onClose}
      footer={[
        <Button key="cancel" onClick={onClose}>
          Cancel
        </Button>,
        <Button
          key="reprocess"
          type="primary"
          danger
          loading={reprocess.isPending}
          onClick={handleReprocess}
        >
          Reprocess
        </Button>,
      ]}
    >
      <Alert
        type="warning"
        showIcon
        message="Reprocessing rebuilds this document's chunks from the original file."
        description="Scanned / vision-OCR'd documents are re-OCR'd and may differ from the current corpus. Review the handling below before re-running."
        style={{ marginBottom: 12 }}
      />

      {previewing ? (
        <div style={{ textAlign: 'center', padding: 24 }}>
          <Spin /> <span style={{ marginLeft: 8 }}>Analyzing…</span>
        </div>
      ) : previewError ? (
        <Alert
          type="info"
          showIcon
          message="Preview unavailable"
          description="The original file isn't stored for this document, so a dry-run preview can't be shown. You can still reprocess with a chosen handling mode."
          style={{ marginBottom: 12 }}
        />
      ) : (
        preview && <DocumentPreviewPanel preview={preview} />
      )}

      <HandlingControls
        handlingMode={handlingMode}
        onHandlingMode={setHandlingMode}
        covThreshold={covThreshold}
        onCovThreshold={setCovThreshold}
        minChars={minChars}
        onMinChars={setMinChars}
        preview={preview}
      />
    </Modal>
  );
}
