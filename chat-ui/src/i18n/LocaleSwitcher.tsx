import { Button, Tooltip } from 'antd';
import { GlobalOutlined } from '@ant-design/icons';
import { useI18n } from './LocaleProvider';

/** One-tap EN ⇄ ไทย toggle for the UI chrome, shown in the sidebar footer next
 *  to the theme picker. The label always previews the language you'd switch TO,
 *  so it's readable whichever language you're stuck in. */
export function LocaleSwitcher() {
  const { locale, setLocale, t } = useI18n();
  const next = locale === 'en' ? 'th' : 'en';
  return (
    <Tooltip title={`${t('language')} — ${next === 'th' ? 'ไทย' : 'English'}`}>
      <Button
        type="text"
        aria-label={t('language')}
        data-testid="locale-switcher"
        onClick={() => setLocale(next)}
        icon={<GlobalOutlined style={{ color: 'var(--ink-icon)' }} />}
      >
        <span style={{ color: 'var(--ink-dim)', fontSize: 12 }}>
          {next === 'th' ? 'ไทย' : 'EN'}
        </span>
      </Button>
    </Tooltip>
  );
}
