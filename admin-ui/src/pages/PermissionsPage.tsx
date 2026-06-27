import { useState } from 'react';
import { Typography, Select, Space, Tour } from 'antd';
import { useOrgs } from '../hooks/useOrgs';
import { PermissionMatrix } from '../components/permissions/PermissionMatrix';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getPermissionsSteps } from '../tours/steps/permissions';

export function PermissionsPage() {
  const [orgId, setOrgId] = useState<string>();
  const orgs = useOrgs();
  const { t } = useI18n();
  const tour = useTour('permissions');

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <div>
          <div className="eyebrow">Access Control</div>
          <Typography.Title level={4} style={{ margin: 0, fontFamily: 'var(--font-display)' }}>
            Permissions
          </Typography.Title>
        </div>
        <TourGuideButton tourId="permissions" />
      </div>
      <Space style={{ marginBottom: 16 }} data-tour="perm-scope">
        <Select
          placeholder="Select Organization"
          style={{ width: 300 }}
          value={orgId}
          onChange={setOrgId}
          options={orgs.data?.data.map((o) => ({ label: o.name, value: o.id }))}
          allowClear
        />
      </Space>

      <div data-tour="perm-table">
        {orgId ? (
          <PermissionMatrix orgId={orgId} />
        ) : (
          <Typography.Text type="secondary">
            Select an organization to manage permissions.
          </Typography.Text>
        )}
      </div>
      <Tour
        open={tour.isActive}
        steps={getPermissionsSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
