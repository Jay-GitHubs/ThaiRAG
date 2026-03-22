import { useEffect, useState } from 'react';
import {
  Card, Collapse, Table, Tag, Button, Modal, Input, Typography, Space, message,
  Popconfirm, Tooltip, Badge,
} from 'antd';
import {
  EditOutlined, UndoOutlined, RobotOutlined,
  MessageOutlined, FileTextOutlined,
} from '@ant-design/icons';
import { listPrompts, updatePrompt, deletePromptOverride } from '../../api/settings';
import type { PromptEntry } from '../../api/types';

const { TextArea } = Input;

const CATEGORY_ICONS: Record<string, React.ReactNode> = {
  chat: <MessageOutlined />,
  document: <FileTextOutlined />,
};

const CATEGORY_COLORS: Record<string, string> = {
  chat: 'blue',
  document: 'green',
};

export function PromptsTab() {
  const [prompts, setPrompts] = useState<PromptEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingPrompt, setEditingPrompt] = useState<PromptEntry | null>(null);
  const [editTemplate, setEditTemplate] = useState('');
  const [editDescription, setEditDescription] = useState('');
  const [saving, setSaving] = useState(false);
  const [filter, setFilter] = useState<string>('all');

  async function loadPrompts() {
    setLoading(true);
    try {
      const data = await listPrompts();
      setPrompts(data);
    } catch {
      message.error('Failed to load prompts');
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { loadPrompts(); }, []);

  async function handleSave() {
    if (!editingPrompt) return;
    setSaving(true);
    try {
      await updatePrompt(editingPrompt.key, {
        template: editTemplate,
        description: editDescription || undefined,
      });
      message.success('Prompt updated');
      setEditingPrompt(null);
      loadPrompts();
    } catch {
      message.error('Failed to update prompt');
    } finally {
      setSaving(false);
    }
  }

  async function handleRevert(key: string) {
    try {
      await deletePromptOverride(key);
      message.success('Prompt reverted to default');
      loadPrompts();
    } catch {
      message.error('Failed to revert prompt');
    }
  }

  const filtered = filter === 'all'
    ? prompts
    : prompts.filter(p => p.category === filter);

  const chatCount = prompts.filter(p => p.category === 'chat').length;
  const docCount = prompts.filter(p => p.category === 'document').length;
  const overrideCount = prompts.filter(p => p.source === 'override').length;

  return (
    <Card
      title={<><RobotOutlined /> Agent System Prompts</>}
      extra={
        <Space>
          <Badge count={overrideCount} style={{ backgroundColor: '#faad14' }} offset={[8, 0]}>
            <Tag color={overrideCount > 0 ? 'orange' : 'default'}>
              {overrideCount} override{overrideCount !== 1 ? 's' : ''}
            </Tag>
          </Badge>
          <Button.Group size="small">
            <Button
              type={filter === 'all' ? 'primary' : 'default'}
              onClick={() => setFilter('all')}
            >
              All ({prompts.length})
            </Button>
            <Button
              type={filter === 'chat' ? 'primary' : 'default'}
              onClick={() => setFilter('chat')}
            >
              Chat ({chatCount})
            </Button>
            <Button
              type={filter === 'document' ? 'primary' : 'default'}
              onClick={() => setFilter('document')}
            >
              Document ({docCount})
            </Button>
          </Button.Group>
        </Space>
      }
    >
      <Typography.Paragraph type="secondary" style={{ marginBottom: 16 }}>
        Edit agent system prompts in real-time. Changes take effect immediately.
        Use <Typography.Text code>{'{{variable}}'}</Typography.Text> syntax for template placeholders.
      </Typography.Paragraph>

      <Collapse
        defaultActiveKey={['prompts-table']}
        items={[{
          key: 'prompts-table',
          label: `Prompts (${filtered.length})`,
          children: (
      <Table<PromptEntry>
        dataSource={filtered}
        rowKey="key"
        loading={loading}
        size="small"
        pagination={{ pageSize: 15, showSizeChanger: true }}
        columns={[
          {
            title: 'Category',
            dataIndex: 'category',
            width: 110,
            filters: [
              { text: 'Chat', value: 'chat' },
              { text: 'Document', value: 'document' },
            ],
            onFilter: (val, r) => r.category === val,
            render: (cat: string) => (
              <Tag icon={CATEGORY_ICONS[cat]} color={CATEGORY_COLORS[cat] || 'default'}>
                {cat}
              </Tag>
            ),
          },
          {
            title: 'Agent',
            dataIndex: 'key',
            width: 250,
            sorter: (a, b) => a.key.localeCompare(b.key),
            render: (key: string, record) => (
              <Space direction="vertical" size={0}>
                <Typography.Text strong code style={{ fontSize: 12 }}>
                  {key}
                </Typography.Text>
                <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                  {record.description}
                </Typography.Text>
              </Space>
            ),
          },
          {
            title: 'Source',
            dataIndex: 'source',
            width: 90,
            filters: [
              { text: 'Default', value: 'default' },
              { text: 'Override', value: 'override' },
            ],
            onFilter: (val, r) => r.source === val,
            render: (source: string) => (
              <Tag color={source === 'override' ? 'orange' : 'default'}>
                {source}
              </Tag>
            ),
          },
          {
            title: 'Preview',
            dataIndex: 'template',
            ellipsis: true,
            render: (template: string) => (
              <Typography.Text
                type="secondary"
                style={{ fontSize: 11, fontFamily: 'monospace' }}
              >
                {template.slice(0, 120)}{template.length > 120 ? '...' : ''}
              </Typography.Text>
            ),
          },
          {
            title: '',
            width: 100,
            render: (_, record) => (
              <Space size="small">
                <Tooltip title="Edit prompt">
                  <Button
                    size="small"
                    icon={<EditOutlined />}
                    onClick={() => {
                      setEditingPrompt(record);
                      setEditTemplate(record.template);
                      setEditDescription(record.description);
                    }}
                  />
                </Tooltip>
                {record.source === 'override' && (
                  <Popconfirm
                    title="Revert to default?"
                    description="This will discard your custom prompt and restore the original."
                    onConfirm={() => handleRevert(record.key)}
                  >
                    <Tooltip title="Revert to default">
                      <Button size="small" icon={<UndoOutlined />} danger />
                    </Tooltip>
                  </Popconfirm>
                )}
              </Space>
            ),
          },
        ]}
      />
          ),
        }]}
      />

      <Modal
        title={
          <Space>
            <EditOutlined />
            Edit Prompt: <Typography.Text code>{editingPrompt?.key}</Typography.Text>
          </Space>
        }
        open={!!editingPrompt}
        onOk={handleSave}
        onCancel={() => setEditingPrompt(null)}
        okText="Save"
        confirmLoading={saving}
        width={800}
        styles={{ body: { paddingTop: 16 } }}
      >
        <Space direction="vertical" style={{ width: '100%' }} size="middle">
          <div>
            <Typography.Text strong style={{ display: 'block', marginBottom: 4 }}>
              Description
            </Typography.Text>
            <Input
              value={editDescription}
              onChange={e => setEditDescription(e.target.value)}
              placeholder="What this prompt does..."
            />
          </div>
          <div>
            <Typography.Text strong style={{ display: 'block', marginBottom: 4 }}>
              Template
            </Typography.Text>
            <Typography.Text type="secondary" style={{ fontSize: 11, display: 'block', marginBottom: 4 }}>
              Use <Typography.Text code>{'{{variable}}'}</Typography.Text> for dynamic values
              that the system fills in at runtime.
            </Typography.Text>
            <TextArea
              value={editTemplate}
              onChange={e => setEditTemplate(e.target.value)}
              rows={18}
              style={{ fontFamily: 'monospace', fontSize: 12 }}
            />
          </div>
          {editingPrompt?.source === 'override' && (
            <Typography.Text type="warning" style={{ fontSize: 12 }}>
              This prompt has been customized. Click "Revert to default" in the table to restore the original.
            </Typography.Text>
          )}
        </Space>
      </Modal>
    </Card>
  );
}
