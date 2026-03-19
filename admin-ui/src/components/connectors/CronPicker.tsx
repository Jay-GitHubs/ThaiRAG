import { useState, useEffect } from 'react';
import { Input, Select, Space, InputNumber, Typography } from 'antd';

const PRESETS = [
  { label: 'Every 15 minutes', value: '0 */15 * * * *' },
  { label: 'Every 30 minutes', value: '0 */30 * * * *' },
  { label: 'Every hour', value: '0 0 * * * *' },
  { label: 'Every 2 hours', value: '0 0 */2 * * *' },
  { label: 'Every 6 hours', value: '0 0 */6 * * *' },
  { label: 'Every 12 hours', value: '0 0 */12 * * *' },
  { label: 'Daily at midnight', value: '0 0 0 * * *' },
  { label: 'Daily at 2:00 AM', value: '0 0 2 * * *' },
  { label: 'Daily at 6:00 AM', value: '0 0 6 * * *' },
  { label: 'Weekly (Sunday midnight)', value: '0 0 0 * * 0' },
  { label: 'Weekly (Monday midnight)', value: '0 0 0 * * 1' },
  { label: 'Monthly (1st at midnight)', value: '0 0 0 1 * *' },
  { label: 'Custom...', value: '__custom__' },
];

/** Convert a 6-field cron expression to a human-readable string. */
export function cronToHuman(cron: string): string {
  const preset = PRESETS.find((p) => p.value === cron);
  if (preset && preset.value !== '__custom__') return preset.label;

  const parts = cron.trim().split(/\s+/);
  if (parts.length < 5) return cron;

  // Support both 5-field (standard) and 6-field (with seconds) cron
  const [sec, min, hour, dom, mon, dow] =
    parts.length === 6 ? parts : ['0', ...parts];

  const pieces: string[] = [];

  // Seconds
  if (sec !== '0') pieces.push(`at second ${sec}`);

  // Minutes
  if (min.startsWith('*/')) {
    return `Every ${min.slice(2)} minutes`;
  }

  // Hours
  if (hour.startsWith('*/')) {
    return `Every ${hour.slice(2)} hours`;
  }

  // Specific time
  if (min !== '*' && hour !== '*' && !hour.startsWith('*/')) {
    const h = parseInt(hour, 10);
    const m = parseInt(min, 10);
    const period = h >= 12 ? 'PM' : 'AM';
    const h12 = h === 0 ? 12 : h > 12 ? h - 12 : h;
    const timeStr = `${h12}:${m.toString().padStart(2, '0')} ${period}`;

    // Day of week
    const days = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
    if (dow !== '*' && dow !== '?') {
      const dayIdx = parseInt(dow, 10);
      const dayName = days[dayIdx] ?? dow;
      // Day of month
      if (dom !== '*' && dom !== '?') {
        return `${dayName}, day ${dom} at ${timeStr}`;
      }
      return `Every ${dayName} at ${timeStr}`;
    }

    // Day of month
    if (dom !== '*' && dom !== '?') {
      const suffix =
        dom === '1' || dom === '21' || dom === '31'
          ? 'st'
          : dom === '2' || dom === '22'
            ? 'nd'
            : dom === '3' || dom === '23'
              ? 'rd'
              : 'th';
      return `Monthly on the ${dom}${suffix} at ${timeStr}`;
    }

    // Month
    if (mon !== '*' && mon !== '?') {
      return `In month ${mon} at ${timeStr}`;
    }

    return `Daily at ${timeStr}`;
  }

  // Fallback: just show the raw expression
  return cron;
}

interface CronPickerProps {
  value?: string;
  onChange?: (value: string) => void;
}

export function CronPicker({ value, onChange }: CronPickerProps) {
  const isCustom = value ? !PRESETS.some((p) => p.value === value) : false;
  const [mode, setMode] = useState<'preset' | 'custom'>(isCustom ? 'custom' : 'preset');
  const [customValue, setCustomValue] = useState(isCustom ? value ?? '' : '');

  useEffect(() => {
    if (value) {
      const found = PRESETS.some((p) => p.value === value);
      if (!found) {
        setMode('custom');
        setCustomValue(value);
      } else {
        setMode('preset');
      }
    }
  }, [value]);

  const handlePresetChange = (v: string) => {
    if (v === '__custom__') {
      setMode('custom');
      setCustomValue(value ?? '');
    } else {
      setMode('preset');
      onChange?.(v);
    }
  };

  const handleCustomChange = (v: string) => {
    setCustomValue(v);
    onChange?.(v);
  };

  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      <Select
        value={mode === 'custom' ? '__custom__' : value}
        onChange={handlePresetChange}
        options={PRESETS}
        style={{ width: '100%' }}
        placeholder="Select schedule..."
      />
      {mode === 'custom' && (
        <>
          <Input
            value={customValue}
            onChange={(e) => handleCustomChange(e.target.value)}
            placeholder="sec min hour day month weekday (e.g. 0 0 2 * * *)"
          />
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            6-field cron: second minute hour day-of-month month day-of-week
          </Typography.Text>
        </>
      )}
      {value && (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          Schedule: {cronToHuman(value)}
        </Typography.Text>
      )}
    </Space>
  );
}
