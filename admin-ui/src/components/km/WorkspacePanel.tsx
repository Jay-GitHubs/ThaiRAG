import { Card, Descriptions, Tabs, Button, Popconfirm, Statistic, message } from 'antd';
import { DeleteOutlined, FileTextOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { getWorkspace } from '../../api/km';
import { useDocuments } from '../../hooks/useDocuments';
import { useDeleteWorkspace } from '../../hooks/useWorkspaces';
import {
  useWorkspacePermissions,
  useGrantWorkspacePermission,
  useRevokeWorkspacePermission,
} from '../../hooks/useScopedPermissions';
import { ScopedPermissions } from './ScopedPermissions';
import type { Role } from '../../api/types';

interface Props {
  orgId: string;
  deptId: string;
  wsId: string;
  onMutated: () => void;
}

export function WorkspacePanel({ orgId, deptId, wsId, onMutated }: Props) {
  const navigate = useNavigate();
  const ws = useQuery({
    queryKey: ['workspace', orgId, deptId, wsId],
    queryFn: () => getWorkspace(orgId, deptId, wsId),
  });
  const docs = useDocuments(wsId);
  const deleteWs = useDeleteWorkspace();

  const perms = useWorkspacePermissions(orgId, deptId, wsId);
  const grantPerm = useGrantWorkspacePermission();
  const revokePerm = useRevokeWorkspacePermission();

  async function handleDelete() {
    try {
      await deleteWs.mutateAsync({ orgId, deptId, wsId });
      onMutated();
    } catch {
      message.error('Failed to delete workspace');
    }
  }

  return (
    <Card
      title={`Workspace: ${ws.data?.name ?? '...'}`}
      loading={ws.isLoading}
      extra={
        <Popconfirm title="Delete this workspace?" onConfirm={handleDelete}>
          <Button danger icon={<DeleteOutlined />}>
            Delete
          </Button>
        </Popconfirm>
      }
    >
      {ws.data && (
        <Descriptions size="small" column={2} style={{ marginBottom: 16 }}>
          <Descriptions.Item label="ID">{ws.data.id}</Descriptions.Item>
          <Descriptions.Item label="Created">
            {dayjs(ws.data.created_at).format('YYYY-MM-DD HH:mm')}
          </Descriptions.Item>
        </Descriptions>
      )}

      <Tabs
        defaultActiveKey="overview"
        items={[
          {
            key: 'overview',
            label: 'Overview',
            children: (
              <>
                <Statistic
                  title="Documents"
                  value={docs.data?.total ?? 0}
                  prefix={<FileTextOutlined />}
                  style={{ marginBottom: 16 }}
                />
                <Button type="primary" onClick={() => navigate(`/documents?ws=${wsId}`)}>
                  Open Documents
                </Button>
              </>
            ),
          },
          {
            key: 'permissions',
            label: 'Permissions',
            children: (
              <ScopedPermissions
                permissions={perms.data?.data}
                loading={perms.isLoading}
                scopeLabel="Workspace"
                onGrant={async (email: string, role: Role) => {
                  await grantPerm.mutateAsync({ orgId, deptId, wsId, data: { email, role } });
                }}
                onRevoke={async (email: string) => {
                  await revokePerm.mutateAsync({ orgId, deptId, wsId, data: { email } });
                }}
              />
            ),
          },
        ]}
      />
    </Card>
  );
}
