import { useState } from 'react';
import {
  Button,
  Card,
  Divider,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
  InputNumber,
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  EditOutlined,
  SafetyCertificateOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  KeyOutlined,
  RobotOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { VaultKeyInfo, LlmProfileInfo } from '../../api/types';
import {
  useVaultKeys,
  useCreateVaultKey,
  useUpdateVaultKey,
  useDeleteVaultKey,
  useTestVaultKey,
  useLlmProfiles,
  useCreateLlmProfile,
  useUpdateLlmProfile,
  useDeleteLlmProfile,
} from '../../hooks/useSettings';

const { Text, Title, Paragraph } = Typography;

const providerColors: Record<string, string> = {
  openai: 'green',
  anthropic: 'purple',
  google: 'gold',
  cohere: 'cyan',
  custom: 'default',
};

const kindOptions = [
  { label: 'Ollama', value: 'Ollama' },
  { label: 'Claude', value: 'Claude' },
  { label: 'OpenAI', value: 'OpenAi' },
  { label: 'Gemini', value: 'Gemini' },
  { label: 'OpenAI-Compatible', value: 'OpenAiCompatible' },
];

// ── Vault Key Modal ──────────────────────────────────────────────────

function VaultKeyModal({
  open,
  editingKey,
  onClose,
  onSubmit,
  submitting,
}: {
  open: boolean;
  editingKey: VaultKeyInfo | null;
  onClose: () => void;
  onSubmit: (values: { name: string; provider: string; api_key: string; base_url: string }) => void;
  submitting: boolean;
}) {
  const [form] = Form.useForm();
  const isEdit = !!editingKey;

  const handleOpen = () => {
    if (editingKey) {
      form.setFieldsValue({
        name: editingKey.name,
        provider: editingKey.provider,
        api_key: '',
        base_url: editingKey.base_url,
      });
    } else {
      form.resetFields();
    }
  };

  return (
    <Modal
      open={open}
      title={isEdit ? 'Edit API Key' : 'Add API Key'}
      onCancel={onClose}
      afterOpenChange={(visible) => visible && handleOpen()}
      onOk={() => form.submit()}
      confirmLoading={submitting}
      okText={isEdit ? 'Update' : 'Create'}
      destroyOnClose
    >
      <Form form={form} layout="vertical" onFinish={onSubmit}>
        <Form.Item name="name" label="Name" rules={[{ required: true, message: 'Name is required' }]}>
          <Input placeholder="e.g., My OpenAI Key" />
        </Form.Item>
        <Form.Item name="provider" label="Provider" rules={[{ required: true }]} initialValue="openai">
          <Select
            options={[
              { label: 'OpenAI', value: 'openai' },
              { label: 'Anthropic', value: 'anthropic' },
              { label: 'Google', value: 'google' },
              { label: 'Cohere', value: 'cohere' },
              { label: 'Custom', value: 'custom' },
            ]}
          />
        </Form.Item>
        <Form.Item
          name="api_key"
          label={isEdit ? 'API Key (leave empty to keep existing)' : 'API Key'}
          rules={isEdit ? [] : [{ required: true, message: 'API key is required' }]}
        >
          <Input.Password placeholder="sk-..." />
        </Form.Item>
        <Form.Item name="base_url" label="Base URL (optional)">
          <Input placeholder="https://api.openai.com/v1 (leave empty for default)" />
        </Form.Item>
      </Form>
    </Modal>
  );
}

// ── LLM Profile Modal ────────────────────────────────────────────────

function LlmProfileModal({
  open,
  editingProfile,
  vaultKeys,
  onClose,
  onSubmit,
  submitting,
}: {
  open: boolean;
  editingProfile: LlmProfileInfo | null;
  vaultKeys: VaultKeyInfo[];
  onClose: () => void;
  onSubmit: (values: {
    name: string;
    kind: string;
    model: string;
    base_url: string;
    vault_key_id?: string;
    max_tokens?: number;
  }) => void;
  submitting: boolean;
}) {
  const [form] = Form.useForm();
  const isEdit = !!editingProfile;

  const handleOpen = () => {
    if (editingProfile) {
      form.setFieldsValue({
        name: editingProfile.name,
        kind: editingProfile.kind,
        model: editingProfile.model,
        base_url: editingProfile.base_url,
        vault_key_id: editingProfile.vault_key_id || undefined,
        max_tokens: editingProfile.max_tokens,
      });
    } else {
      form.resetFields();
    }
  };

  const kind = Form.useWatch('kind', form);
  const needsKey = kind && kind !== 'Ollama';

  return (
    <Modal
      open={open}
      title={isEdit ? 'Edit LLM Profile' : 'Create LLM Profile'}
      onCancel={onClose}
      afterOpenChange={(visible) => visible && handleOpen()}
      onOk={() => form.submit()}
      confirmLoading={submitting}
      okText={isEdit ? 'Update' : 'Create'}
      destroyOnClose
    >
      <Form form={form} layout="vertical" onFinish={onSubmit}>
        <Form.Item name="name" label="Profile Name" rules={[{ required: true }]}>
          <Input placeholder="e.g., GPT-4.1 for Chat" />
        </Form.Item>
        <Form.Item name="kind" label="Provider Kind" rules={[{ required: true }]} initialValue="Ollama">
          <Select options={kindOptions} />
        </Form.Item>
        <Form.Item name="model" label="Model" rules={[{ required: true }]}>
          <Input placeholder="e.g., gpt-4.1, claude-sonnet-4-20250514" />
        </Form.Item>
        <Form.Item name="base_url" label="Base URL (optional)">
          <Input placeholder="Leave empty for default" />
        </Form.Item>
        {needsKey && (
          <Form.Item name="vault_key_id" label="API Key">
            <Select
              allowClear
              placeholder="Select a vault key"
              options={vaultKeys.map((k) => ({
                label: `${k.name} (${k.key_masked})`,
                value: k.id,
              }))}
            />
          </Form.Item>
        )}
        <Form.Item name="max_tokens" label="Max Tokens (optional)">
          <InputNumber min={1} max={1000000} style={{ width: '100%' }} placeholder="Default from provider" />
        </Form.Item>
      </Form>
    </Modal>
  );
}

// ── Main Component ──────────────────────────────────────────────────

export function VaultTab() {
  // Vault keys
  const { data: vaultKeys, isLoading: keysLoading } = useVaultKeys();
  const createKey = useCreateVaultKey();
  const updateKey = useUpdateVaultKey();
  const deleteKey = useDeleteVaultKey();
  const testKey = useTestVaultKey();

  const [keyModalOpen, setKeyModalOpen] = useState(false);
  const [editingKey, setEditingKey] = useState<VaultKeyInfo | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);

  // LLM profiles
  const { data: profiles, isLoading: profilesLoading } = useLlmProfiles();
  const createProfile = useCreateLlmProfile();
  const updateProfile = useUpdateLlmProfile();
  const deleteProfile = useDeleteLlmProfile();

  const [profileModalOpen, setProfileModalOpen] = useState(false);
  const [editingProfile, setEditingProfile] = useState<LlmProfileInfo | null>(null);

  // ── Vault Key handlers ────────────────────────────────────────────

  const handleCreateKey = () => {
    setEditingKey(null);
    setKeyModalOpen(true);
  };

  const handleEditKey = (key: VaultKeyInfo) => {
    setEditingKey(key);
    setKeyModalOpen(true);
  };

  const handleKeySubmit = async (values: { name: string; provider: string; api_key: string; base_url: string }) => {
    if (editingKey) {
      const data: { name?: string; api_key?: string; base_url?: string } = { name: values.name };
      if (values.api_key) data.api_key = values.api_key;
      if (values.base_url !== undefined) data.base_url = values.base_url;
      await updateKey.mutateAsync({ id: editingKey.id, data });
      message.success('API key updated');
    } else {
      await createKey.mutateAsync(values);
      message.success('API key created');
    }
    setKeyModalOpen(false);
  };

  const handleTestKey = async (id: string) => {
    setTestingId(id);
    try {
      const result = await testKey.mutateAsync(id);
      if (result.status === 'ok') {
        message.success(result.message || 'Key is valid');
      } else {
        message.error(result.message || 'Key test failed');
      }
    } catch {
      message.error('Failed to test key');
    } finally {
      setTestingId(null);
    }
  };

  const handleDeleteKey = async (id: string) => {
    await deleteKey.mutateAsync(id);
    message.success('API key deleted');
  };

  // ── LLM Profile handlers ─────────────────────────────────────────

  const handleCreateProfile = () => {
    setEditingProfile(null);
    setProfileModalOpen(true);
  };

  const handleEditProfile = (profile: LlmProfileInfo) => {
    setEditingProfile(profile);
    setProfileModalOpen(true);
  };

  const handleProfileSubmit = async (values: {
    name: string;
    kind: string;
    model: string;
    base_url: string;
    vault_key_id?: string;
    max_tokens?: number;
  }) => {
    if (editingProfile) {
      await updateProfile.mutateAsync({
        id: editingProfile.id,
        data: {
          name: values.name,
          kind: values.kind,
          model: values.model,
          base_url: values.base_url,
          vault_key_id: values.vault_key_id,
          remove_vault_key: !values.vault_key_id ? true : undefined,
          max_tokens: values.max_tokens,
        },
      });
      message.success('Profile updated');
    } else {
      await createProfile.mutateAsync(values);
      message.success('Profile created');
    }
    setProfileModalOpen(false);
  };

  const handleDeleteProfile = async (id: string) => {
    await deleteProfile.mutateAsync(id);
    message.success('Profile deleted');
  };

  // ── Table columns ─────────────────────────────────────────────────

  const keyColumns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name: string) => <Text strong>{name}</Text>,
    },
    {
      title: 'Provider',
      dataIndex: 'provider',
      key: 'provider',
      render: (provider: string) => (
        <Tag color={providerColors[provider] || 'default'}>{provider}</Tag>
      ),
    },
    {
      title: 'Key',
      dataIndex: 'key_masked',
      key: 'key_masked',
      render: (masked: string) => <Text code>{masked}</Text>,
    },
    {
      title: 'Endpoint',
      dataIndex: 'base_url',
      key: 'base_url',
      render: (url: string) => url || <Text type="secondary">Default</Text>,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (d: string) => dayjs(d).format('YYYY-MM-DD'),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: VaultKeyInfo) => (
        <Space>
          <Button
            size="small"
            icon={<SafetyCertificateOutlined />}
            loading={testingId === record.id}
            onClick={() => handleTestKey(record.id)}
          >
            Test
          </Button>
          <Button size="small" icon={<EditOutlined />} onClick={() => handleEditKey(record)}>
            Edit
          </Button>
          <Popconfirm
            title="Delete this API key?"
            description="LLM profiles using this key may stop working."
            onConfirm={() => handleDeleteKey(record.id)}
          >
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const profileColumns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name: string) => <Text strong>{name}</Text>,
    },
    {
      title: 'Provider & Model',
      key: 'kind_model',
      render: (_: unknown, r: LlmProfileInfo) => (
        <Space>
          <Tag color="blue">{r.kind}</Tag>
          <Text code>{r.model}</Text>
        </Space>
      ),
    },
    {
      title: 'API Key',
      key: 'vault_key',
      render: (_: unknown, r: LlmProfileInfo) =>
        r.vault_key_name ? (
          <Tag icon={<KeyOutlined />} color="green">{r.vault_key_name}</Tag>
        ) : r.kind === 'Ollama' ? (
          <Text type="secondary">Local</Text>
        ) : (
          <Tag color="warning">No key</Tag>
        ),
    },
    {
      title: 'Max Tokens',
      dataIndex: 'max_tokens',
      key: 'max_tokens',
      render: (v: number | undefined) => v ?? <Text type="secondary">Default</Text>,
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: unknown, record: LlmProfileInfo) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => handleEditProfile(record)}>
            Edit
          </Button>
          <Popconfirm
            title="Delete this profile?"
            description="Agents using this profile will fall back to inline config."
            onConfirm={() => handleDeleteProfile(record.id)}
          >
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      {/* Section 1: API Keys */}
      <Card
        title={
          <Space>
            <KeyOutlined />
            <span>API Keys</span>
          </Space>
        }
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={handleCreateKey}>
            Add API Key
          </Button>
        }
      >
        <Paragraph type="secondary">
          Store API keys securely with AES-256-GCM encryption. Keys are encrypted at rest and never exposed in logs or API responses.
        </Paragraph>
        <Table
          dataSource={vaultKeys || []}
          columns={keyColumns}
          rowKey="id"
          loading={keysLoading}
          size="small"
          pagination={false}
          locale={{ emptyText: 'No API keys stored. Add one to get started.' }}
        />
      </Card>

      {/* Section 2: LLM Profiles */}
      <Card
        title={
          <Space>
            <RobotOutlined />
            <span>LLM Profiles</span>
          </Space>
        }
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={handleCreateProfile}>
            Create Profile
          </Button>
        }
      >
        <Paragraph type="secondary">
          Create reusable LLM configurations that combine a provider, model, and API key.
          Assign profiles to chat agents or document processing agents instead of configuring each one manually.
        </Paragraph>
        <Table
          dataSource={profiles || []}
          columns={profileColumns}
          rowKey="id"
          loading={profilesLoading}
          size="small"
          pagination={false}
          locale={{ emptyText: 'No LLM profiles. Create one to simplify agent configuration.' }}
        />
      </Card>

      {/* Modals */}
      <VaultKeyModal
        open={keyModalOpen}
        editingKey={editingKey}
        onClose={() => setKeyModalOpen(false)}
        onSubmit={handleKeySubmit}
        submitting={createKey.isPending || updateKey.isPending}
      />
      <LlmProfileModal
        open={profileModalOpen}
        editingProfile={editingProfile}
        vaultKeys={vaultKeys || []}
        onClose={() => setProfileModalOpen(false)}
        onSubmit={handleProfileSubmit}
        submitting={createProfile.isPending || updateProfile.isPending}
      />
    </Space>
  );
}
