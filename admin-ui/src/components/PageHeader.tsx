import type { ReactNode } from 'react';
import { Typography } from 'antd';

/**
 * Standard page header: a mono eyebrow over a display-font title, with optional
 * inline adornment (children, e.g. a tour button) and right-aligned actions.
 * The title stays an accessible <h4> heading so existing selectors hold.
 */
export function PageHeader({
  eyebrow,
  title,
  extra,
  children,
}: {
  eyebrow?: string;
  title: string;
  extra?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-end',
        justifyContent: 'space-between',
        gap: 12,
        flexWrap: 'wrap',
        marginBottom: 20,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
        <div style={{ minWidth: 0 }}>
          {eyebrow && (
            <div className="eyebrow" style={{ marginBottom: 2 }}>
              {eyebrow}
            </div>
          )}
          <Typography.Title level={4} style={{ margin: 0, fontFamily: 'var(--font-display)' }}>
            {title}
          </Typography.Title>
        </div>
        {children}
      </div>
      {extra && <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>{extra}</div>}
    </div>
  );
}
