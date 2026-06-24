import { Radio, InputNumber, Space, Tooltip } from 'antd';
import type { DocumentHandling, DocumentPreview } from '../../api/types';

interface Props {
  handlingMode: DocumentHandling['handling_mode'];
  onHandlingMode: (m: DocumentHandling['handling_mode']) => void;
  covThreshold: number | null;
  onCovThreshold: (v: number | null) => void;
  minChars: number | null;
  onMinChars: (v: number | null) => void;
  /** Optional preview, whose thresholds become the input placeholders. */
  preview?: DocumentPreview | null;
}

/** Per-document handling override controls — the admin's choice for a single
 *  ingest/reprocess: handling mode + optional threshold tweaks. Shared by the
 *  upload modal (first ingest) and the reprocess modal (re-run with options) so
 *  both speak the exact same levers. */
export function HandlingControls({
  handlingMode,
  onHandlingMode,
  covThreshold,
  onCovThreshold,
  minChars,
  onMinChars,
  preview,
}: Props) {
  return (
    <div style={{ marginTop: 16 }}>
      <div style={{ fontSize: 12, color: '#888', marginBottom: 6 }}>Handling</div>
      <Radio.Group
        value={handlingMode}
        onChange={(e) => onHandlingMode(e.target.value)}
        optionType="button"
        size="small"
      >
        <Tooltip title="Adaptive routing (recommended)">
          <Radio.Button value="auto">Auto</Radio.Button>
        </Tooltip>
        <Tooltip title="OCR every page via the vision model — max fidelity, slowest">
          <Radio.Button value="high_quality">High quality</Radio.Button>
        </Tooltip>
        <Tooltip title="Deterministic OCR tier only — no vision LLM (no hallucination)">
          <Radio.Button value="force_ocr">OCR only</Radio.Button>
        </Tooltip>
        <Tooltip title="No models — text layer only (fast, zero cost/risk)">
          <Radio.Button value="text_only">Text only</Radio.Button>
        </Tooltip>
      </Radio.Group>
      <Space size="large" style={{ marginTop: 10, display: 'flex', flexWrap: 'wrap' }}>
        <span style={{ fontSize: 12 }}>
          <Tooltip title="Override the image-coverage threshold for this document (blank = default)">
            <span style={{ color: '#888', marginRight: 6 }}>Image-coverage ≥</span>
          </Tooltip>
          <InputNumber
            size="small"
            min={0}
            max={1}
            step={0.05}
            value={covThreshold}
            placeholder={preview ? String(preview.thresholds.image_coverage_threshold) : '0.5'}
            onChange={(v) => onCovThreshold(v ?? null)}
            style={{ width: 90 }}
          />
        </span>
        <span style={{ fontSize: 12 }}>
          <Tooltip title="Override the min-chars/page threshold for this document (blank = default)">
            <span style={{ color: '#888', marginRight: 6 }}>Min chars/page</span>
          </Tooltip>
          <InputNumber
            size="small"
            min={0}
            max={100000}
            value={minChars}
            placeholder={preview ? String(preview.thresholds.min_chars_per_page) : '50'}
            onChange={(v) => onMinChars(v ?? null)}
            style={{ width: 90 }}
          />
        </span>
      </Space>
    </div>
  );
}
