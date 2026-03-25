import { Select, Space, Tag, Typography } from 'antd';
import { GlobalOutlined, BankOutlined, TeamOutlined, AppstoreOutlined } from '@ant-design/icons';
import { useOrgs } from '../../hooks/useOrgs';
import { useDepts } from '../../hooks/useDepts';
import { useWorkspaces } from '../../hooks/useWorkspaces';
import type { SettingsScopeParam } from '../../api/types';

const { Text } = Typography;

interface ScopeSelectorProps {
  value: SettingsScopeParam | undefined;
  onChange: (scope: SettingsScopeParam | undefined) => void;
}

export function ScopeSelector({ value, onChange }: ScopeSelectorProps) {
  const { data: orgsData } = useOrgs();
  const orgs = orgsData?.data ?? [];

  // Track selection state for cascading
  const scopeType = value?.scope_type;
  const selectedOrgId =
    scopeType === 'org' ? value?.scope_id
    : scopeType === 'dept' || scopeType === 'workspace' ? undefined // resolved from dept/ws
    : undefined;

  // For dept/workspace, we need the org context — store locally
  // The scope selector is simpler: pick level, then pick entity
  const { data: deptsData } = useDepts(orgs.length === 1 ? orgs[0].id : undefined);
  const depts = deptsData?.data ?? [];

  const firstOrg = orgs[0];
  const firstDept = depts[0];
  const { data: wsData } = useWorkspaces(
    firstOrg?.id,
    firstDept?.id,
  );
  const workspaces = wsData?.data ?? [];

  // Build flat options list: Global, then each org, dept, workspace
  const options: { label: React.ReactNode; value: string }[] = [
    {
      label: (
        <Space>
          <GlobalOutlined />
          <span>Global (Default)</span>
        </Space>
      ),
      value: 'global:',
    },
  ];

  for (const org of orgs) {
    options.push({
      label: (
        <Space>
          <BankOutlined />
          <span>Org: {org.name}</span>
        </Space>
      ),
      value: `org:${org.id}`,
    });
  }

  // If there's exactly one org, show its depts and workspaces inline
  if (orgs.length === 1 && depts.length > 0) {
    for (const dept of depts) {
      options.push({
        label: (
          <Space>
            <TeamOutlined />
            <span>Dept: {dept.name}</span>
          </Space>
        ),
        value: `dept:${dept.id}`,
      });
    }
  }

  if (orgs.length === 1 && depts.length === 1 && workspaces.length > 0) {
    for (const ws of workspaces) {
      options.push({
        label: (
          <Space>
            <AppstoreOutlined />
            <span>Workspace: {ws.name}</span>
          </Space>
        ),
        value: `workspace:${ws.id}`,
      });
    }
  }

  const currentValue = value ? `${value.scope_type}:${value.scope_id}` : 'global:';

  const handleChange = (val: string) => {
    const [type, id] = val.split(':');
    if (type === 'global') {
      onChange(undefined);
    } else {
      onChange({ scope_type: type, scope_id: id });
    }
  };

  const scopeLabel = !value ? 'Global' : value.scope_type === 'org' ? 'Organization' : value.scope_type === 'dept' ? 'Department' : 'Workspace';
  const scopeColor = !value ? 'default' : value.scope_type === 'org' ? 'blue' : value.scope_type === 'dept' ? 'green' : 'orange';

  return (
    <Space>
      <Text strong>Settings Scope:</Text>
      <Select
        value={currentValue}
        onChange={handleChange}
        style={{ minWidth: 250 }}
        options={options}
      />
      <Tag color={scopeColor}>{scopeLabel}</Tag>
      {value && (
        <Text type="secondary">
          Overrides at this level take precedence over parent scopes
        </Text>
      )}
    </Space>
  );
}
