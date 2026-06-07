import { Modal } from 'antd';
import { useDocument } from '../../hooks/useDocuments';
import { ProcessingTimeline } from './ProcessingTimeline';
import type { Document } from '../../api/types';

interface Props {
  workspaceId: string;
  doc: Document;
  open: boolean;
  onClose: () => void;
}

/**
 * Re-openable processing detail: renders the same per-stage tracker shown during
 * upload, for any document. While the document is still processing it polls live
 * (spinners, elapsed time, current model); once finished it shows the persisted
 * timeline + provenance + fidelity — a permanent, re-openable record so a user
 * who closed the upload dialog can always get back to the detail.
 */
export function ProcessingDetailModal({ workspaceId, doc, open, onClose }: Props) {
  const { data: live } = useDocument(workspaceId, doc.id, open);
  return (
    <Modal title="Processing details" open={open} onCancel={onClose} footer={null} width={560}>
      <ProcessingTimeline doc={live ?? doc} />
    </Modal>
  );
}
