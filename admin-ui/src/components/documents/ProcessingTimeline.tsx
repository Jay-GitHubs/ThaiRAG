import { useEffect, useState, type ReactNode } from 'react';
import { Steps, Tag, Typography, Alert, Space, Tooltip } from 'antd';
import {
  LoadingOutlined,
  CheckCircleFilled,
  CloseCircleFilled,
  CloudUploadOutlined,
  ThunderboltFilled,
} from '@ant-design/icons';
import type { Document, StageTiming } from '../../api/types';

const { Text } = Typography;

// Map a raw pipeline step name (e.g. "converting_with_vision",
// "orchestrator_reviewing_chunks", "retrying_chunking_1") onto a coarse,
// user-facing phase. Order matters — first match wins.
const PHASE_MATCHERS: { test: (s: string) => boolean; key: string; label: string }[] = [
  { test: (s) => s.includes('analy'), key: 'analyze', label: 'Analyze' },
  { test: (s) => s.includes('convert') || s.includes('conversion'), key: 'convert', label: 'Convert' },
  { test: (s) => s.includes('quality'), key: 'quality', label: 'Quality Check' },
  { test: (s) => s.includes('chunk'), key: 'chunk', label: 'Chunk' },
  { test: (s) => s.includes('enrich'), key: 'enrich', label: 'Enrich' },
  { test: (s) => s.includes('index'), key: 'index', label: 'Index & Embed' },
];

// Which AI agent backs each phase — used to attribute the model that ran the
// stage (sourced from processing_provenance, populated as the pipeline finishes).
const PHASE_AGENT: Record<string, string> = {
  analyze: 'analyzer',
  convert: 'converter',
  quality: 'quality',
  chunk: 'chunker',
  enrich: 'enricher',
};

// The algorithm/method each stage uses — the "what" shown alongside the model.
// `convert` is resolved dynamically (vision OCR vs. text conversion).
function methodForPhase(key: string, usedVision: boolean, mechanical: boolean): string | undefined {
  switch (key) {
    case 'analyze':
      return 'LLM analysis';
    case 'convert':
      return usedVision ? 'vision OCR' : 'LLM conversion';
    case 'quality':
      return 'LLM evaluation';
    case 'chunk':
      return mechanical ? 'mechanical chunking' : 'Smart Chunker (LLM)';
    case 'enrich':
      return 'LLM enrichment';
    case 'index':
      return 'vector embedding';
    default:
      return undefined;
  }
}

function phaseOf(step: string): { key: string; label: string } {
  const m = PHASE_MATCHERS.find((p) => p.test(step));
  if (m) return { key: m.key, label: m.label };
  return {
    key: step,
    label: step.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase()),
  };
}

interface PhaseAgg {
  key: string;
  label: string;
  closedMs: number; // sum of completed step durations
  open: boolean; // a step in this phase is still running
  openStartedMs?: number;
  stepCount: number; // raw steps mapped here (retries inflate this)
  usedVision: boolean;
  model?: string; // model running this phase, recorded live on the timeline
}

function aggregate(timeline: StageTiming[]): PhaseAgg[] {
  const order: string[] = [];
  const map = new Map<string, PhaseAgg>();
  for (const st of timeline) {
    const ph = phaseOf(st.step);
    if (!map.has(ph.key)) {
      map.set(ph.key, {
        key: ph.key,
        label: ph.label,
        closedMs: 0,
        open: false,
        stepCount: 0,
        usedVision: false,
      });
      order.push(ph.key);
    }
    const agg = map.get(ph.key)!;
    agg.stepCount += 1;
    if (st.step.includes('vision')) agg.usedVision = true;
    if (st.model) agg.model = st.model;
    if (st.duration_ms == null) {
      agg.open = true;
      agg.openStartedMs = st.started_at_ms;
    } else {
      agg.closedMs += st.duration_ms;
    }
  }
  return order.map((k) => map.get(k)!);
}

function fmt(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(s < 10 ? 1 : 0)}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${Math.round(s - m * 60)}s`;
}

interface Props {
  doc: Document;
}

/**
 * Live, per-stage processing tracker. Renders the document's
 * `processing_timeline` as a vertical step list — each stage shows a spinner
 * while running and its elapsed time once done — so the user can see exactly
 * where the pipeline is and which stage is the bottleneck. Drives off the
 * polled `doc`; an internal 1s ticker keeps the active stage's timer live
 * between polls.
 */
export function ProcessingTimeline({ doc }: Props) {
  const [now, setNow] = useState(() => Date.now());
  const processing = doc.status === 'processing';

  useEffect(() => {
    if (!processing) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [processing]);

  const timeline = doc.processing_timeline ?? [];
  const phases = aggregate(timeline);

  // The model that actually ran each agent, keyed by agent id. Populated from
  // provenance once the pipeline records it (at/near completion).
  const agentRuns = new Map<string, { model?: string; status: string; note?: string }>();
  for (const a of doc.processing_provenance?.agents ?? []) agentRuns.set(a.agent, a);

  // Effective duration per phase (closed + live elapsed for the open one).
  const effMs = (p: PhaseAgg) =>
    p.closedMs + (p.open && p.openStartedMs ? Math.max(0, now - p.openStartedMs) : 0);

  // Highlight the slowest phase as the bottleneck once processing has settled.
  const bottleneckKey =
    doc.status === 'ready' && phases.length > 1
      ? phases.reduce((a, b) => (effMs(b) > effMs(a) ? b : a)).key
      : null;

  type ItemStatus = 'finish' | 'process' | 'error' | 'wait';
  const items: {
    title: ReactNode;
    description?: ReactNode;
    status: ItemStatus;
    icon?: ReactNode;
  }[] = [];

  // Leading node — the upload itself always succeeded by the time we're here.
  items.push({
    title: 'Uploaded',
    description: <Text type="secondary">{doc.title}</Text>,
    status: 'finish',
    icon: <CloudUploadOutlined />,
  });

  const failed = doc.status === 'failed';
  const lastIdx = phases.length - 1;

  phases.forEach((p, i) => {
    const isOpen = p.open && processing;
    const isFailedHere = failed && i === lastIdx;
    const status: ItemStatus = isFailedHere ? 'error' : isOpen ? 'process' : 'finish';
    const ms = effMs(p);
    const tags: ReactNode[] = [];
    if (p.key === bottleneckKey) {
      tags.push(
        <Tag key="bn" color="volcano" icon={<ThunderboltFilled />}>
          bottleneck
        </Tag>,
      );
    }
    if (p.usedVision) tags.push(<Tag key="v" color="purple">vision</Tag>);
    if (p.stepCount > 1) tags.push(<Tag key="r">{p.stepCount} passes</Tag>);

    // Per-stage attribution: method (algorithm) + the model running it. Both
    // show LIVE — the model is stamped on each timeline entry as the stage
    // starts (provenance is used only as a fallback / for agent status).
    const run = agentRuns.get(PHASE_AGENT[p.key]);
    const model = p.model ?? run?.model;
    const method = methodForPhase(p.key, p.usedVision, !!doc.processing_provenance?.mechanical_fallback);
    const statusSuffix =
      run?.status === 'failed' ? ' (failed)' : run?.status === 'skipped' ? ' (skipped)' : '';
    const description =
      method || model ? (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {method}
          {model ? (
            <>
              {method ? ' · ' : ''}
              <span style={{ fontFamily: 'monospace' }}>
                {model}
                {statusSuffix}
              </span>
            </>
          ) : (
            statusSuffix
          )}
        </Text>
      ) : undefined;

    items.push({
      title: (
        <Space size={6}>
          <span>{p.label}</span>
          <Text type="secondary" style={{ fontVariantNumeric: 'tabular-nums' }}>
            {isOpen ? `${fmt(ms)}…` : fmt(ms)}
          </Text>
          {tags}
        </Space>
      ),
      description,
      status,
      icon: isOpen ? <LoadingOutlined /> : undefined,
    });
  });

  // Trailing terminal node.
  if (doc.status === 'ready') {
    const totalMs = phases.reduce((sum, p) => sum + p.closedMs, 0);
    items.push({
      title: 'Ready',
      description: (
        <Text type="secondary">
          {doc.chunk_count} chunks indexed
          {totalMs > 0 ? ` · ${fmt(totalMs)} total` : ''}
        </Text>
      ),
      status: 'finish',
      icon: <CheckCircleFilled style={{ color: 'var(--success)' }} />,
    });
  } else if (failed) {
    items.push({
      title: 'Failed',
      status: 'error',
      icon: <CloseCircleFilled style={{ color: 'var(--danger)' }} />,
    });
  } else if (phases.length === 0) {
    // Processing accepted but no stage reported yet.
    items.push({
      title: 'Starting…',
      description: <Text type="secondary">Queued for processing</Text>,
      status: 'process',
      icon: <LoadingOutlined />,
    });
  } else {
    items.push({ title: 'Indexing…', status: 'wait' });
  }

  return (
    <div>
      <Steps direction="vertical" size="small" items={items} />
      {failed && doc.error_message && (
        <Alert
          type="error"
          showIcon
          style={{ marginTop: 12 }}
          message="Processing failed"
          description={<Text code>{doc.error_message}</Text>}
        />
      )}
      {doc.status === 'ready' && doc.processing_provenance && (
        <Alert
          type="success"
          showIcon={false}
          style={{ marginTop: 12 }}
          message={
            <Space size={8} wrap>
              <Text strong>Path:</Text>
              <Tag color="geekblue">{doc.processing_provenance.path}</Tag>
              {doc.processing_provenance.mechanical_fallback && (
                <Tag color="warning">mechanical fallback</Tag>
              )}
              {!!doc.processing_provenance.tables_kept_as_text && (
                <Tooltip
                  title="Tabular page(s) had no clean grid to reconstruct deterministically. The raw text was kept verbatim (numbers exact) instead of risking vision OCR — the table structure may need a manual look."
                >
                  <Tag color="warning">
                    {doc.processing_provenance.tables_kept_as_text} table
                    {doc.processing_provenance.tables_kept_as_text > 1 ? 's' : ''} kept as text
                  </Tag>
                </Tooltip>
              )}
            </Space>
          }
        />
      )}
    </div>
  );
}
