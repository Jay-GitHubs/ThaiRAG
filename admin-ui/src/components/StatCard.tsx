import type { ReactNode } from 'react';
import { Card, Spin } from 'antd';

/**
 * A single metric: an accent icon chip beside a mono eyebrow label and a large
 * display-font value. Themed (accent defaults to the brand accent). The label is
 * plain text so it stays queryable.
 */
export function StatCard({
  label,
  value,
  icon,
  accent = 'var(--celadon)',
  tint = 'var(--celadon-tint)',
  loading = false,
  footer,
}: {
  label: string;
  value: ReactNode;
  icon: ReactNode;
  accent?: string;
  tint?: string;
  loading?: boolean;
  footer?: ReactNode;
}) {
  return (
    <Card size="small" style={{ height: '100%' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
        <span
          aria-hidden
          style={{
            width: 42,
            height: 42,
            borderRadius: 10,
            flexShrink: 0,
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            background: tint,
            color: accent,
            fontSize: 20,
          }}
        >
          {icon}
        </span>
        <div style={{ minWidth: 0 }}>
          <div className="eyebrow" style={{ marginBottom: 3 }}>
            {label}
          </div>
          <div
            style={{
              fontFamily: 'var(--font-display)',
              fontSize: 26,
              fontWeight: 600,
              lineHeight: 1.1,
              color: 'var(--text)',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {loading ? <Spin size="small" /> : value}
          </div>
          {footer && (
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 2 }}>{footer}</div>
          )}
        </div>
      </div>
    </Card>
  );
}
