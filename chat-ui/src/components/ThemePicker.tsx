import { useState } from 'react';
import { useI18n } from '../i18n/LocaleProvider';
import { Button, Popover, Tooltip } from 'antd';
import { BgColorsOutlined, CheckOutlined } from '@ant-design/icons';
import { useTheme, type ThemeDef } from '../theme/ThemeProvider';

/** Sidebar control to pick one of the app's themes. A popover of two-color
 *  swatches grouped Light / Dark, with the active theme checked. */
export function ThemePicker() {
  const { t } = useI18n();
  const { themeId, themes, setTheme } = useTheme();
  const [open, setOpen] = useState(false);

  const groups: Array<'Light' | 'Dark'> = ['Light', 'Dark'];

  const row = (t: ThemeDef) => {
    const active = t.id === themeId;
    return (
      <button
        key={t.id}
        type="button"
        data-testid={`theme-option-${t.id}`}
        onClick={() => {
          setTheme(t.id);
          setOpen(false);
        }}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          width: '100%',
          border: 'none',
          background: active ? 'var(--celadon-tint)' : 'transparent',
          borderRadius: 8,
          padding: '7px 9px',
          cursor: 'pointer',
          textAlign: 'left',
          color: 'var(--text)',
        }}
      >
        <span
          aria-hidden
          style={{
            position: 'relative',
            width: 30,
            height: 20,
            borderRadius: 5,
            flexShrink: 0,
            background: t.swatch.canvas,
            border: '1px solid var(--line)',
            overflow: 'hidden',
          }}
        >
          <span
            style={{
              position: 'absolute',
              right: 0,
              top: 0,
              bottom: 0,
              width: 12,
              background: t.swatch.accent,
            }}
          />
        </span>
        <span style={{ flex: 1, fontSize: 13.5 }}>{t.label}</span>
        {active && <CheckOutlined style={{ color: 'var(--celadon-deep)', fontSize: 12 }} />}
      </button>
    );
  };

  const content = (
    <div data-testid="theme-menu" style={{ width: 210 }}>
      {groups.map((g) => (
        <div key={g} style={{ marginBottom: 6 }}>
          <div className="eyebrow" style={{ padding: '4px 9px 2px' }}>
            {g}
          </div>
          {themes.filter((t) => t.group === g).map(row)}
        </div>
      ))}
    </div>
  );

  return (
    <Popover
      content={content}
      trigger="click"
      open={open}
      onOpenChange={setOpen}
      placement="topRight"
    >
      <Tooltip title={t('theme')}>
        <Button
          type="text"
          aria-label={t('chooseTheme')}
          data-testid="theme-picker"
          icon={<BgColorsOutlined style={{ color: 'var(--ink-icon)' }} />}
        />
      </Tooltip>
    </Popover>
  );
}
