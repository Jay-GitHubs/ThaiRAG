import { Modal, Form, Select, message } from 'antd';
import { useDepts } from '../../hooks/useDepts';
import { useWorkspaces } from '../../hooks/useWorkspaces';
import { useGrantPermission } from '../../hooks/usePermissions';
import { useUsers } from '../../hooks/useUsers';
import { useState } from 'react';
import type { Role, ScopeRequest } from '../../api/types';

interface Props {
  orgId: string;
  open: boolean;
  onClose: () => void;
}

const roleOptions: { label: string; value: Role }[] = [
  { label: 'Owner', value: 'owner' },
  { label: 'Admin', value: 'admin' },
  { label: 'Editor', value: 'editor' },
  { label: 'Viewer', value: 'viewer' },
];

export function GrantModal({ orgId, open, onClose }: Props) {
  const [form] = Form.useForm();
  const [scopeLevel, setScopeLevel] = useState<'Org' | 'Dept' | 'Workspace'>('Org');
  const [deptId, setDeptId] = useState<string>();
  const grant = useGrantPermission();
  const users = useUsers();
  const depts = useDepts(orgId);
  const workspaces = useWorkspaces(orgId, deptId);

  async function handleOk() {
    try {
      const values = await form.validateFields();
      let scope: ScopeRequest;
      if (scopeLevel === 'Workspace') {
        scope = { level: 'Workspace', dept_id: values.dept_id, workspace_id: values.workspace_id };
      } else if (scopeLevel === 'Dept') {
        scope = { level: 'Dept', dept_id: values.dept_id };
      } else {
        scope = { level: 'Org' };
      }

      await grant.mutateAsync({
        orgId,
        data: { email: values.email, role: values.role, scope },
      });
      message.success('Permission granted');
      form.resetFields();
      setScopeLevel('Org');
      setDeptId(undefined);
      onClose();
    } catch (err: unknown) {
      const msg =
        err && typeof err === 'object' && 'response' in err
          ? (err as { response: { data?: { error?: { message?: string } } } }).response?.data
              ?.error?.message
          : undefined;
      message.error(msg || 'Failed to grant permission');
    }
  }

  return (
    <Modal
      title="Grant Permission"
      open={open}
      onOk={handleOk}
      onCancel={onClose}
      confirmLoading={grant.isPending}
    >
      <Form form={form} layout="vertical" initialValues={{ role: 'viewer' }}>
        <Form.Item name="email" label="User" rules={[{ required: true }]}>
          <Select
            showSearch
            placeholder="Select a user"
            optionFilterProp="label"
            loading={users.isLoading}
            options={users.data?.data.map((u) => ({
              label: `${u.name} (${u.email})`,
              value: u.email,
            }))}
          />
        </Form.Item>
        <Form.Item name="role" label="Role" rules={[{ required: true }]}>
          <Select options={roleOptions} />
        </Form.Item>
        <Form.Item label="Scope Level">
          <Select
            value={scopeLevel}
            onChange={(v) => {
              setScopeLevel(v);
              if (v === 'Org') {
                form.setFieldsValue({ dept_id: undefined, workspace_id: undefined });
                setDeptId(undefined);
              }
            }}
            options={[
              { label: 'Organization', value: 'Org' },
              { label: 'Department', value: 'Dept' },
              { label: 'Workspace', value: 'Workspace' },
            ]}
          />
        </Form.Item>
        {(scopeLevel === 'Dept' || scopeLevel === 'Workspace') && (
          <Form.Item name="dept_id" label="Department" rules={[{ required: true }]}>
            <Select
              placeholder="Select department"
              options={depts.data?.data.map((d) => ({ label: d.name, value: d.id }))}
              onChange={(v) => setDeptId(v)}
            />
          </Form.Item>
        )}
        {scopeLevel === 'Workspace' && (
          <Form.Item name="workspace_id" label="Workspace" rules={[{ required: true }]}>
            <Select
              placeholder="Select workspace"
              options={workspaces.data?.data.map((w) => ({ label: w.name, value: w.id }))}
              disabled={!deptId}
            />
          </Form.Item>
        )}
      </Form>
    </Modal>
  );
}
