import { useEffect, useState } from 'react';
import { Form, Input, Modal, Select } from 'antd';
import type { Connector } from '../../api/types';
import { useOrgs } from '../../hooks/useOrgs';
import { useDepts } from '../../hooks/useDepts';
import { useWorkspaces } from '../../hooks/useWorkspaces';
import { CronPicker } from './CronPicker';

interface Props {
  open: boolean;
  editingConnector: Connector | null;
  onCancel: () => void;
  onSubmit: (values: Record<string, unknown>) => Promise<void>;
  loading?: boolean;
}

export function ConnectorFormModal({
  open,
  editingConnector,
  onCancel,
  onSubmit,
  loading,
}: Props) {
  const [form] = Form.useForm();
  const transport = Form.useWatch('transport', form);
  const syncMode = Form.useWatch('sync_mode', form);

  // Cascading workspace selector state
  const [orgId, setOrgId] = useState<string>();
  const [deptId, setDeptId] = useState<string>();
  const orgs = useOrgs();
  const depts = useDepts(orgId);
  const workspaces = useWorkspaces(orgId, deptId);

  useEffect(() => {
    if (open) {
      if (editingConnector) {
        form.setFieldsValue({
          name: editingConnector.name,
          description: editingConnector.description,
          transport: editingConnector.transport,
          command: editingConnector.command,
          args: editingConnector.args.join(' '),
          url: editingConnector.url,
          sync_mode: editingConnector.sync_mode,
          schedule_cron: editingConnector.schedule_cron,
          resource_filters: editingConnector.resource_filters.join('\n'),
          max_items_per_sync: editingConnector.max_items_per_sync,
          webhook_url: editingConnector.webhook_url,
        });
      } else {
        form.resetFields();
        form.setFieldsValue({ transport: 'stdio', sync_mode: 'on_demand' });
        setOrgId(undefined);
        setDeptId(undefined);
      }
    }
  }, [open, editingConnector, form]);

  const handleOk = async () => {
    const values = await form.validateFields();
    if (typeof values.args === 'string') {
      values.args = values.args.split(/\s+/).filter((s: string) => s.length > 0);
    }
    if (typeof values.resource_filters === 'string') {
      values.resource_filters = values.resource_filters
        .split('\n')
        .map((s: string) => s.trim())
        .filter((s: string) => s.length > 0);
    }
    await onSubmit(values);
  };

  return (
    <Modal
      title={editingConnector ? 'Edit Connector' : 'Create Connector'}
      open={open}
      onOk={handleOk}
      onCancel={onCancel}
      confirmLoading={loading}
      width={640}
      destroyOnClose
    >
      <Form form={form} layout="vertical">
        <Form.Item
          name="name"
          label="Name"
          rules={[{ required: true, message: 'Name is required' }]}
        >
          <Input placeholder="e.g. My Confluence" />
        </Form.Item>

        <Form.Item name="description" label="Description">
          <Input.TextArea rows={2} placeholder="Optional description" />
        </Form.Item>

        {!editingConnector && (
          <>
            <Form.Item label="Organization" required>
              <Select
                placeholder="Select organization"
                value={orgId}
                onChange={(v) => {
                  setOrgId(v);
                  setDeptId(undefined);
                  form.setFieldValue('workspace_id', undefined);
                }}
                options={(orgs.data?.data ?? []).map((o) => ({
                  label: o.name,
                  value: o.id,
                }))}
                allowClear
              />
            </Form.Item>
            <Form.Item label="Department" required>
              <Select
                placeholder="Select department"
                disabled={!orgId}
                value={deptId}
                onChange={(v) => {
                  setDeptId(v);
                  form.setFieldValue('workspace_id', undefined);
                }}
                options={(depts.data?.data ?? []).map((d) => ({
                  label: d.name,
                  value: d.id,
                }))}
                allowClear
              />
            </Form.Item>
            <Form.Item
              name="workspace_id"
              label="Target Workspace"
              rules={[{ required: true, message: 'Workspace is required' }]}
            >
              <Select
                placeholder="Select workspace"
                disabled={!deptId}
                options={(workspaces.data?.data ?? []).map((ws) => ({
                  label: ws.name,
                  value: ws.id,
                }))}
                showSearch
                optionFilterProp="label"
              />
            </Form.Item>
          </>
        )}

        <Form.Item name="transport" label="Transport" rules={[{ required: true }]}>
          <Select
            options={[
              { label: 'Stdio (local process)', value: 'stdio' },
              { label: 'SSE (remote server)', value: 'sse' },
            ]}
          />
        </Form.Item>

        {transport === 'stdio' && (
          <>
            <Form.Item
              name="command"
              label="Command"
              rules={[{ required: true, message: 'Command is required for stdio' }]}
            >
              <Input placeholder="e.g. npx" />
            </Form.Item>
            <Form.Item name="args" label="Arguments (space-separated)">
              <Input placeholder="e.g. -y @modelcontextprotocol/server-filesystem /data" />
            </Form.Item>
          </>
        )}

        {transport === 'sse' && (
          <Form.Item
            name="url"
            label="Server URL"
            rules={[{ required: true, message: 'URL is required for SSE' }]}
          >
            <Input placeholder="e.g. http://localhost:3001/sse" />
          </Form.Item>
        )}

        <Form.Item name="sync_mode" label="Sync Mode">
          <Select
            options={[
              { label: 'On Demand', value: 'on_demand' },
              { label: 'Scheduled', value: 'scheduled' },
            ]}
          />
        </Form.Item>

        {syncMode === 'scheduled' && (
          <Form.Item
            name="schedule_cron"
            label="Schedule"
            rules={[{ required: true, message: 'Schedule is required for scheduled sync' }]}
          >
            <CronPicker />
          </Form.Item>
        )}

        <Form.Item name="resource_filters" label="Resource Filters (one per line)">
          <Input.TextArea rows={2} placeholder="Optional URI patterns to include" />
        </Form.Item>

        <Form.Item name="max_items_per_sync" label="Max Items Per Sync">
          <Input type="number" placeholder="Leave empty for unlimited" />
        </Form.Item>

        <Form.Item name="webhook_url" label="Webhook URL">
          <Input placeholder="Optional URL to notify after sync" />
        </Form.Item>

        {!editingConnector && (
          <Form.Item name="webhook_secret" label="Webhook Secret">
            <Input.Password placeholder="Optional bearer token for webhook" />
          </Form.Item>
        )}
      </Form>
    </Modal>
  );
}
