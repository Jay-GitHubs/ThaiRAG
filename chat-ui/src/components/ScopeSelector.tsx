import { Select } from 'antd';
import { DatabaseOutlined } from '@ant-design/icons';
import type { WorkspaceOption } from '../api/types';

/**
 * Picks the workspace a new conversation is scoped to. Pinning to one workspace
 * hard-filters retrieval ("one product per scope"), which avoids near-clone
 * cross-contamination. `null` = search across all of the user's workspaces.
 */
export function ScopeSelector({
  workspaces,
  value,
  onChange,
  disabled,
  size = 'middle',
}: {
  workspaces: WorkspaceOption[];
  value: string | null;
  onChange: (v: string | null) => void;
  disabled?: boolean;
  size?: 'small' | 'middle' | 'large';
}) {
  return (
    <Select
      value={value ?? 'all'}
      onChange={(v) => onChange(v === 'all' ? null : v)}
      disabled={disabled}
      size={size}
      style={{ minWidth: 230 }}
      suffixIcon={<DatabaseOutlined />}
      showSearch
      optionFilterProp="label"
      options={[
        { value: 'all', label: 'All my workspaces' },
        ...workspaces.map((w) => ({ value: w.id, label: w.name })),
      ]}
    />
  );
}
