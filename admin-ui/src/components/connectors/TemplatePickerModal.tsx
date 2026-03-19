import { useState } from 'react';
import { Card, Col, Form, Input, Modal, Row, Select, Typography } from 'antd';
import type { ConnectorTemplate } from '../../api/types';
import { useOrgs } from '../../hooks/useOrgs';
import { useDepts } from '../../hooks/useDepts';
import { useWorkspaces } from '../../hooks/useWorkspaces';
import { CronPicker } from './CronPicker';

interface Props {
  open: boolean;
  templates: ConnectorTemplate[];
  onCancel: () => void;
  onSubmit: (values: {
    template_id: string;
    workspace_id: string;
    name?: string;
    env: Record<string, string>;
    sync_mode: string;
    schedule_cron?: string;
  }) => Promise<void>;
  loading?: boolean;
}

export function TemplatePickerModal({
  open,
  templates,
  onCancel,
  onSubmit,
  loading,
}: Props) {
  const [selected, setSelected] = useState<ConnectorTemplate | null>(null);
  const [form] = Form.useForm();
  const syncMode = Form.useWatch('sync_mode', form);

  // Cascading workspace selector
  const [orgId, setOrgId] = useState<string>();
  const [deptId, setDeptId] = useState<string>();
  const orgs = useOrgs();
  const depts = useDepts(orgId);
  const workspaces = useWorkspaces(orgId, deptId);

  const handleSelect = (t: ConnectorTemplate) => {
    setSelected(t);
    form.resetFields();
    form.setFieldsValue({ name: t.name, sync_mode: 'on_demand' });
    setOrgId(undefined);
    setDeptId(undefined);
  };

  const handleOk = async () => {
    if (!selected) return;
    const values = await form.validateFields();
    const env: Record<string, string> = {};
    for (const key of selected.env_keys) {
      if (values[`env_${key}`]) {
        env[key] = values[`env_${key}`];
      }
    }
    await onSubmit({
      template_id: selected.id,
      workspace_id: values.workspace_id,
      name: values.name,
      env,
      sync_mode: values.sync_mode,
      schedule_cron: values.schedule_cron,
    });
    setSelected(null);
  };

  const handleCancel = () => {
    setSelected(null);
    setOrgId(undefined);
    setDeptId(undefined);
    onCancel();
  };

  if (!selected) {
    return (
      <Modal
        title="Choose a Template"
        open={open}
        onCancel={handleCancel}
        footer={null}
        width={720}
      >
        <Row gutter={[16, 16]}>
          {templates.map((t) => (
            <Col xs={24} sm={12} md={8} key={t.id}>
              <Card
                hoverable
                onClick={() => handleSelect(t)}
                style={{ height: '100%' }}
              >
                <Typography.Text strong>{t.name}</Typography.Text>
                <br />
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {t.description}
                </Typography.Text>
              </Card>
            </Col>
          ))}
        </Row>
      </Modal>
    );
  }

  return (
    <Modal
      title={`Create from "${selected.name}" Template`}
      open={open}
      onOk={handleOk}
      onCancel={handleCancel}
      confirmLoading={loading}
      width={560}
    >
      <Form form={form} layout="vertical">
        <Form.Item name="name" label="Connector Name">
          <Input placeholder={selected.name} />
        </Form.Item>

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

        {selected.env_keys.map((key) => (
          <Form.Item
            key={key}
            name={`env_${key}`}
            label={key}
            rules={[{ required: true, message: `${key} is required` }]}
          >
            <Input.Password placeholder={`Enter ${key}`} />
          </Form.Item>
        ))}

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
            rules={[{ required: true }]}
          >
            <CronPicker />
          </Form.Item>
        )}
      </Form>
    </Modal>
  );
}
