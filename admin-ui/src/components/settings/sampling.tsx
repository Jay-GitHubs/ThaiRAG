import { Collapse, Input, InputNumber, Select, Space, Tooltip, Typography } from 'antd';
import { QuestionCircleOutlined } from '@ant-design/icons';
import type { LlmConfigUpdate, LlmProviderInfo } from '../../api/types';

const { Text } = Typography;

/**
 * Advanced sampling parameters shared by every LLM config editor (primary LLM,
 * document vision LLM, per-agent LLMs). Mirrors the backend `SamplingParams`:
 * unset means "do not send, provider default". Each provider only supports a
 * subset, so fields the selected provider cannot use are disabled with a note —
 * the backend never sends them either.
 */
export interface SamplingFormState {
  top_p?: number;
  top_k?: number;
  min_p?: number;
  repeat_penalty?: number;
  frequency_penalty?: number;
  presence_penalty?: number;
  seed?: number;
  stop: string[];
  /** JSON object text; '' = none. Kept as text so a half-typed value never crashes the form. */
  extra_body: string;
}

export const emptySampling = (): SamplingFormState => ({ stop: [], extra_body: '' });

type NumericKey = 'top_p' | 'top_k' | 'min_p' | 'repeat_penalty' | 'frequency_penalty' | 'presence_penalty' | 'seed';
const NUMERIC_KEYS: NumericKey[] = ['top_p', 'top_k', 'min_p', 'repeat_penalty', 'frequency_penalty', 'presence_penalty', 'seed'];

export function samplingFromInfo(info?: Partial<LlmProviderInfo> | null): SamplingFormState {
  if (!info) return emptySampling();
  return {
    top_p: info.top_p,
    top_k: info.top_k,
    min_p: info.min_p,
    repeat_penalty: info.repeat_penalty,
    frequency_penalty: info.frequency_penalty,
    presence_penalty: info.presence_penalty,
    seed: info.seed,
    stop: info.stop ? [...info.stop] : [],
    extra_body: info.extra_body ? JSON.stringify(info.extra_body, null, 2) : '',
  };
}

export function isSamplingEmpty(s: SamplingFormState): boolean {
  return NUMERIC_KEYS.every((k) => s[k] == null) && s.stop.length === 0 && !s.extra_body.trim();
}

/** Which fields each provider kind can use (matches the backend mapping). */
export function samplingSupport(kind: string): Record<NumericKey | 'stop' | 'extra_body', boolean> {
  const all = { top_p: true, top_k: true, min_p: true, repeat_penalty: true, frequency_penalty: true, presence_penalty: true, seed: true, stop: true, extra_body: true };
  switch (kind) {
    case 'OpenAi':
      return { ...all, top_k: false, min_p: false, repeat_penalty: false };
    case 'Claude':
      return { ...all, min_p: false, repeat_penalty: false, frequency_penalty: false, presence_penalty: false, seed: false };
    case 'Gemini':
      return { ...all, min_p: false, repeat_penalty: false };
    default:
      return all; // Ollama, OpenAiCompatible (vLLM / LiteLLM)
  }
}

/** Parse the extra_body text. Returns undefined for empty, null for invalid. */
export function parseExtraBody(text: string): Record<string, unknown> | undefined | null {
  if (!text.trim()) return undefined;
  try {
    const v = JSON.parse(text);
    return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** First user-facing validation error, or null. Ranges mirror the backend. */
export function samplingError(s: SamplingFormState): string | null {
  const between = (name: string, v: number | undefined, lo: number, hi: number) =>
    v != null && (v < lo || v > hi) ? `${name} must be between ${lo} and ${hi}` : null;
  return (
    between('top_p', s.top_p, 0, 1) ??
    between('min_p', s.min_p, 0, 1) ??
    between('frequency_penalty', s.frequency_penalty, -2, 2) ??
    between('presence_penalty', s.presence_penalty, -2, 2) ??
    (s.top_k != null && s.top_k < 1 ? 'top_k must be at least 1' : null) ??
    (s.repeat_penalty != null && s.repeat_penalty < 0 ? 'repeat_penalty must be ≥ 0' : null) ??
    (s.stop.length > 8 ? 'at most 8 stop sequences' : null) ??
    (parseExtraBody(s.extra_body) === null ? 'extra_body must be a JSON object' : null)
  );
}

/**
 * The update payload for a form's sampling block. The API has one reset flag
 * (`clear_sampling`) rather than per-field clears, so whenever the block is
 * sent it is sent whole: reset, then every set field. Returns `{}` when
 * nothing changed against `prev`.
 */
export function samplingToUpdate(next: SamplingFormState, prev?: SamplingFormState): Partial<LlmConfigUpdate> {
  if (prev && sameSampling(next, prev)) return {};
  const out: Partial<LlmConfigUpdate> = { clear_sampling: true };
  for (const k of NUMERIC_KEYS) {
    if (next[k] != null) out[k] = next[k];
  }
  if (next.stop.length > 0) out.stop = next.stop;
  const extra = parseExtraBody(next.extra_body);
  if (extra) out.extra_body = extra;
  return out;
}

function sameSampling(a: SamplingFormState, b: SamplingFormState): boolean {
  if (!NUMERIC_KEYS.every((k) => (a[k] ?? null) === (b[k] ?? null))) return false;
  if (a.stop.length !== b.stop.length || a.stop.some((v, i) => v !== b.stop[i])) return false;
  return JSON.stringify(parseExtraBody(a.extra_body) ?? null) === JSON.stringify(parseExtraBody(b.extra_body) ?? null);
}

interface FieldSpec {
  key: NumericKey;
  label: string;
  hint: string;
  min?: number;
  max?: number;
  step?: number;
}

const FIELDS: FieldSpec[] = [
  { key: 'top_p', label: 'Top-p', hint: 'Nucleus sampling, 0–1. Claude uses either temperature or top-p, never both (temperature wins).', min: 0, max: 1, step: 0.05 },
  { key: 'top_k', label: 'Top-k', hint: 'Consider only the k most likely tokens (≥ 1). Not part of the OpenAI API; sent to gateways, Claude, Gemini, Ollama.', min: 1, step: 1 },
  { key: 'min_p', label: 'Min-p', hint: 'Drop tokens below this fraction of the top probability, 0–1. vLLM / Ollama only.', min: 0, max: 1, step: 0.01 },
  { key: 'repeat_penalty', label: 'Repeat penalty', hint: '1.0 = off, higher discourages repetition. Ollama repeat_penalty / vLLM repetition_penalty.', min: 0, step: 0.05 },
  { key: 'frequency_penalty', label: 'Frequency penalty', hint: 'OpenAI-style, −2..2. Penalises tokens by how often they already appeared.', min: -2, max: 2, step: 0.1 },
  { key: 'presence_penalty', label: 'Presence penalty', hint: 'OpenAI-style, −2..2. Penalises tokens that appeared at all.', min: -2, max: 2, step: 0.1 },
  { key: 'seed', label: 'Seed', hint: 'Fixed seed for reproducible output where the backend honours it (vLLM, Ollama, Gemini; OpenAI best effort). Pair with temperature 0.', min: 0, step: 1 },
];

interface Props {
  kind: string;
  value: SamplingFormState;
  onChange: (next: SamplingFormState) => void;
  compact?: boolean;
  /** Render the fields directly (no collapse). Default: inside an "Advanced sampling" collapse. */
  inline?: boolean;
}

export function AdvancedSamplingFields({ kind, value, onChange, compact, inline }: Props) {
  const support = samplingSupport(kind);
  const size = compact ? 'small' : 'middle';
  const error = samplingError(value);
  const labelWidth = 120;

  const row = (label: string, hint: string, supported: boolean, control: React.ReactNode) => (
    <Space align="center" wrap key={label}>
      <Tooltip title={supported ? hint : `${hint} Not supported by ${kind}; not sent.`}>
        <Text type={supported ? undefined : 'secondary'} style={{ fontSize: 12, width: labelWidth, display: 'inline-block' }}>
          {label} <QuestionCircleOutlined />
        </Text>
      </Tooltip>
      {control}
    </Space>
  );

  const fields = (
    <Space direction="vertical" size={6} style={{ width: '100%' }}>
      {FIELDS.map((f) =>
        row(
          f.label,
          f.hint,
          support[f.key],
          <InputNumber
            size={size}
            min={f.min}
            max={f.max}
            step={f.step}
            disabled={!support[f.key]}
            value={value[f.key] ?? null}
            onChange={(v) => onChange({ ...value, [f.key]: v == null ? undefined : Number(v) })}
            placeholder="default"
            style={{ width: 130 }}
            data-testid={`sampling-${f.key}`}
          />,
        ),
      )}
      {row(
        'Stop sequences',
        'Generation stops when any of these appears (up to 8). Type and press Enter.',
        support.stop,
        <Select
          size={size}
          mode="tags"
          disabled={!support.stop}
          value={value.stop}
          onChange={(v) => onChange({ ...value, stop: (v as string[]).slice(0, 8) })}
          placeholder="none"
          tokenSeparators={[]}
          style={{ minWidth: 220, maxWidth: 420 }}
          data-testid="sampling-stop"
        />,
      )}
      {row(
        'Extra body (JSON)',
        'Merged verbatim into the request — the escape hatch for gateway-specific knobs, e.g. {"chat_template_kwargs": {"enable_thinking": false}} for Qwen3 on vLLM. Ollama: merged into options; Gemini: into generationConfig.',
        support.extra_body,
        <Input.TextArea
          size={size}
          disabled={!support.extra_body}
          value={value.extra_body}
          onChange={(e) => onChange({ ...value, extra_body: e.target.value })}
          placeholder='{"chat_template_kwargs": {"enable_thinking": false}}'
          autoSize={{ minRows: 1, maxRows: 6 }}
          style={{ width: 420, fontFamily: 'var(--font-mono, monospace)', fontSize: 12 }}
          data-testid="sampling-extra-body"
        />,
      )}
      {error && (
        <Text type="danger" style={{ fontSize: 12 }} data-testid="sampling-error">
          {error}
        </Text>
      )}
      {!isSamplingEmpty(value) && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          <a onClick={() => onChange(emptySampling())}>Reset all to provider defaults</a>
        </Text>
      )}
    </Space>
  );

  if (inline) return fields;
  return (
    <Collapse
      ghost
      size="small"
      items={[
        {
          key: 'sampling',
          label: (
            <Text type="secondary" style={{ fontSize: 12 }}>
              Advanced sampling{isSamplingEmpty(value) ? '' : ' (set)'}
            </Text>
          ),
          children: fields,
        },
      ]}
    />
  );
}
