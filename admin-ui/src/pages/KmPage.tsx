import { useState, useEffect } from 'react';
import { Row, Col, Typography } from 'antd';
import { KmTree } from '../components/km/KmTree';
import { OrgPanel } from '../components/km/OrgPanel';
import { DeptPanel } from '../components/km/DeptPanel';
import { WorkspacePanel } from '../components/km/WorkspacePanel';

export interface KmSelection {
  type: 'org' | 'dept' | 'workspace';
  orgId: string;
  orgName?: string;
  deptId?: string;
  deptName?: string;
  wsId?: string;
  wsName?: string;
}

export function KmPage() {
  const [selection, setSelection] = useState<KmSelection | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  const onMutated = () => setRefreshKey((k) => k + 1);

  useEffect(() => {
    // Reset selection on refresh
  }, [refreshKey]);

  return (
    <>
      <Typography.Title level={4}>KM Hierarchy</Typography.Title>
      <Row gutter={16}>
        <Col xs={24} md={8} lg={6}>
          <KmTree onSelect={setSelection} refreshKey={refreshKey} onMutated={onMutated} />
        </Col>
        <Col xs={24} md={16} lg={18}>
          {!selection && (
            <Typography.Text type="secondary">Select an item from the tree</Typography.Text>
          )}
          {selection?.type === 'org' && (
            <OrgPanel orgId={selection.orgId} onMutated={onMutated} />
          )}
          {selection?.type === 'dept' && selection.deptId && (
            <DeptPanel
              orgId={selection.orgId}
              deptId={selection.deptId}
              onMutated={onMutated}
            />
          )}
          {selection?.type === 'workspace' && selection.deptId && selection.wsId && (
            <WorkspacePanel
              orgId={selection.orgId}
              deptId={selection.deptId}
              wsId={selection.wsId}
              onMutated={onMutated}
            />
          )}
        </Col>
      </Row>
    </>
  );
}
