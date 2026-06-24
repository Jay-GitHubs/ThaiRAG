import { Alert, Tag, Tooltip } from 'antd';
import type { DocumentPreview } from '../../api/types';

/** Visual breakdown of a dry-run document-complexity preview: what the pipeline
 *  WOULD do (per-region classes, fidelity-tier split, thresholds) so an admin can
 *  review the handling decision before processing. */
export function DocumentPreviewPanel({ preview }: { preview: DocumentPreview }) {
  const { total_regions, native_regions, deterministic_ocr_regions, vision_llm_regions } = preview;
  const pct = (n: number) => (total_regions > 0 ? (100 * n) / total_regions : 0);

  // Fully-deterministic = green/info; any model-needing = warning tone.
  const needsModel = deterministic_ocr_regions + vision_llm_regions;

  const classColor: Record<string, string> = {
    NativeText: 'green',
    NativeTable: 'green',
    TabularAsText: 'lime',
    NativeStruct: 'green',
    Mixed: 'blue',
    CorruptedText: 'orange',
    Scanned: 'orange',
    ImageHeavy: 'gold',
    DirectImage: 'purple',
    Unsupported: 'red',
  };

  return (
    <div style={{ marginTop: 12 }}>
      <Alert
        type={needsModel === 0 ? 'success' : 'info'}
        showIcon
        message={`Handling preview — ${preview.format.toUpperCase()}, ${total_regions} region(s)`}
        description={preview.recommendation}
        style={{ marginBottom: 12 }}
      />

      {/* Fidelity-tier split */}
      <div style={{ marginBottom: 8, fontSize: 12, color: '#888' }}>Fidelity routing</div>
      <div style={{ display: 'flex', height: 22, borderRadius: 4, overflow: 'hidden', marginBottom: 4 }}>
        {native_regions > 0 && (
          <Tooltip title={`${native_regions} native (deterministic, no model)`}>
            <div style={{ width: `${pct(native_regions)}%`, background: '#52c41a' }} />
          </Tooltip>
        )}
        {deterministic_ocr_regions > 0 && (
          <Tooltip title={`${deterministic_ocr_regions} deterministic OCR`}>
            <div style={{ width: `${pct(deterministic_ocr_regions)}%`, background: '#faad14' }} />
          </Tooltip>
        )}
        {vision_llm_regions > 0 && (
          <Tooltip title={`${vision_llm_regions} vision LLM`}>
            <div style={{ width: `${pct(vision_llm_regions)}%`, background: '#722ed1' }} />
          </Tooltip>
        )}
      </div>
      <div style={{ display: 'flex', gap: 16, fontSize: 12, marginBottom: 12 }}>
        <span><Tag color="green">Native</Tag>{native_regions}</span>
        <span><Tag color="gold">Det. OCR</Tag>{deterministic_ocr_regions}</span>
        <span><Tag color="purple">Vision LLM</Tag>{vision_llm_regions}</span>
        {!preview.ocr_tier_available && deterministic_ocr_regions > 0 && (
          <span style={{ color: '#fa8c16' }}>⚠ OCR tier not configured</span>
        )}
      </div>

      {/* Per-class counts */}
      <div style={{ marginBottom: 6, fontSize: 12, color: '#888' }}>Region classes</div>
      <div style={{ marginBottom: 12 }}>
        {Object.entries(preview.classes)
          .sort((a, b) => b[1] - a[1])
          .map(([cls, n]) => (
            <Tag key={cls} color={classColor[cls] ?? 'default'} style={{ marginBottom: 4 }}>
              {cls}: {n} ({pct(n).toFixed(0)}%)
            </Tag>
          ))}
      </div>

      {/* Thresholds behind the decision */}
      <div style={{ fontSize: 11, color: '#999' }}>
        Thresholds: image-coverage ≥ {preview.thresholds.image_coverage_threshold}, min-chars/page{' '}
        {preview.thresholds.min_chars_per_page}, garble-ratio ≥ {preview.thresholds.garble_ratio_threshold}
      </div>
    </div>
  );
}
