import { useState, useEffect, useCallback } from 'react';
import {
  Typography,
  Card,
  Button,
  Input,
  Select,
  Modal,
  Form,
  Tag,
  Space,
  Row,
  Col,
  Rate,
  Popconfirm,
  message,
  Spin,
  Tooltip,
  Badge,
  Divider,
} from 'antd';
import {
  PlusOutlined,
  ForkOutlined,
  CopyOutlined,
  DeleteOutlined,
  EyeOutlined,
  EditOutlined,
  SearchOutlined,
  GlobalOutlined,
  LockOutlined,
} from '@ant-design/icons';
import {
  listPromptTemplates,
  createPromptTemplate,
  updatePromptTemplate,
  deletePromptTemplate,
  ratePromptTemplate,
  forkPromptTemplate,
} from '../api/promptMarketplace';
import type { PromptTemplate, CreateTemplateRequest } from '../api/promptMarketplace';
import { useI18n } from '../i18n';
import { useAuth } from '../auth/useAuth';

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;
const { Option } = Select;

const CATEGORIES = [
  'general',
  'rag',
  'summarization',
  'qa',
  'extraction',
  'classification',
  'translation',
  'coding',
  'creative',
];

const CATEGORY_COLORS: Record<string, string> = {
  general: 'default',
  rag: 'blue',
  summarization: 'cyan',
  qa: 'green',
  extraction: 'orange',
  classification: 'purple',
  translation: 'magenta',
  coding: 'geekblue',
  creative: 'volcano',
};

export default function PromptMarketplacePage() {
  const { t } = useI18n();
  const { user } = useAuth();
  const [templates, setTemplates] = useState<PromptTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState('');
  const [categoryFilter, setCategoryFilter] = useState<string | undefined>(undefined);
  const [viewOpen, setViewOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [selected, setSelected] = useState<PromptTemplate | null>(null);
  const [createForm] = Form.useForm();
  const [editForm] = Form.useForm();
  const [submitting, setSubmitting] = useState(false);
  const [variableInput, setVariableInput] = useState('');
  const [variables, setVariables] = useState<string[]>([]);
  const [editVariables, setEditVariables] = useState<string[]>([]);
  const [editVariableInput, setEditVariableInput] = useState('');

  const fetchTemplates = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listPromptTemplates({
        search: search || undefined,
        category: categoryFilter,
      });
      setTemplates(data);
    } catch {
      message.error(t('prompts.loadFailed'));
    } finally {
      setLoading(false);
    }
  }, [search, categoryFilter, t]);

  useEffect(() => {
    fetchTemplates();
  }, [fetchTemplates]);

  const handleCreate = async (values: Record<string, unknown>) => {
    setSubmitting(true);
    try {
      const req: CreateTemplateRequest = {
        name: values.name as string,
        description: values.description as string | undefined,
        category: values.category as string | undefined,
        content: values.content as string,
        variables,
        is_public: values.is_public as boolean ?? true,
      };
      await createPromptTemplate(req);
      message.success(t('prompts.createSuccess'));
      setCreateOpen(false);
      createForm.resetFields();
      setVariables([]);
      setVariableInput('');
      fetchTemplates();
    } catch {
      message.error(t('prompts.createFailed'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleUpdate = async (values: Record<string, unknown>) => {
    if (!selected) return;
    setSubmitting(true);
    try {
      await updatePromptTemplate(selected.id, {
        name: values.name as string,
        description: values.description as string | undefined,
        category: values.category as string | undefined,
        content: values.content as string,
        variables: editVariables,
        is_public: values.is_public as boolean,
      });
      message.success(t('prompts.updateSuccess'));
      setEditOpen(false);
      setSelected(null);
      fetchTemplates();
    } catch {
      message.error(t('prompts.updateFailed'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deletePromptTemplate(id);
      message.success(t('prompts.deleteSuccess'));
      fetchTemplates();
    } catch {
      message.error(t('prompts.deleteFailed'));
    }
  };

  const handleRate = async (templateId: string, rating: number) => {
    try {
      await ratePromptTemplate(templateId, rating);
      message.success(t('prompts.rateSuccess'));
      fetchTemplates();
    } catch {
      message.error(t('prompts.rateFailed'));
    }
  };

  const handleFork = async (id: string) => {
    try {
      const forked = await forkPromptTemplate(id);
      message.success(t('prompts.forkSuccess'));
      fetchTemplates();
      // Open the forked template for editing
      setSelected(forked);
      editForm.setFieldsValue({
        name: forked.name,
        description: forked.description,
        category: forked.category,
        content: forked.content,
        is_public: forked.is_public,
      });
      setEditVariables(forked.variables);
      setEditOpen(true);
    } catch {
      message.error(t('prompts.forkFailed'));
    }
  };

  const handleCopyContent = (content: string) => {
    navigator.clipboard.writeText(content).then(() => {
      message.success(t('prompts.copied'));
    });
  };

  const openView = (template: PromptTemplate) => {
    setSelected(template);
    setViewOpen(true);
  };

  const openEdit = (template: PromptTemplate) => {
    setSelected(template);
    editForm.setFieldsValue({
      name: template.name,
      description: template.description,
      category: template.category,
      content: template.content,
      is_public: template.is_public,
    });
    setEditVariables([...template.variables]);
    setEditOpen(true);
  };

  const addVariable = () => {
    const v = variableInput.trim();
    if (v && !variables.includes(v)) {
      setVariables([...variables, v]);
    }
    setVariableInput('');
  };

  const addEditVariable = () => {
    const v = editVariableInput.trim();
    if (v && !editVariables.includes(v)) {
      setEditVariables([...editVariables, v]);
    }
    setEditVariableInput('');
  };

  const isOwner = (template: PromptTemplate) => {
    return user && (user.is_super_admin || template.author_id === user.id);
  };

  const isAdmin = user?.is_super_admin || user?.role === 'super_admin' || user?.role === 'admin';

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={2} style={{ margin: 0 }}>
          {t('prompts.title')}
        </Title>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
          {t('prompts.create')}
        </Button>
      </div>

      {/* Filters */}
      <Row gutter={[16, 16]} style={{ marginBottom: 16 }}>
        <Col xs={24} sm={12} md={10}>
          <Input
            placeholder={t('prompts.searchPlaceholder')}
            prefix={<SearchOutlined />}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            allowClear
          />
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Select
            placeholder={t('prompts.filterCategory')}
            value={categoryFilter}
            onChange={setCategoryFilter}
            allowClear
            style={{ width: '100%' }}
          >
            {CATEGORIES.map((cat) => (
              <Option key={cat} value={cat}>
                {cat}
              </Option>
            ))}
          </Select>
        </Col>
      </Row>

      <Spin spinning={loading}>
        {templates.length === 0 && !loading ? (
          <Card>
            <Text type="secondary">{t('prompts.noTemplates')}</Text>
          </Card>
        ) : (
          <Row gutter={[16, 16]}>
            {templates.map((template) => (
              <Col xs={24} sm={12} lg={8} xl={6} key={template.id}>
                <Card
                  hoverable
                  style={{ height: '100%', display: 'flex', flexDirection: 'column' }}
                  actions={[
                    <Tooltip title={t('prompts.view')} key="view">
                      <EyeOutlined onClick={() => openView(template)} />
                    </Tooltip>,
                    <Tooltip title={t('prompts.fork')} key="fork">
                      <ForkOutlined onClick={() => handleFork(template.id)} />
                    </Tooltip>,
                    ...(isOwner(template) || isAdmin
                      ? [
                          <Tooltip title={t('prompts.edit')} key="edit">
                            <EditOutlined onClick={() => openEdit(template)} />
                          </Tooltip>,
                          <Tooltip title={t('prompts.delete')} key="delete">
                            <Popconfirm
                              title={t('prompts.deleteConfirm')}
                              onConfirm={() => handleDelete(template.id)}
                            >
                              <DeleteOutlined style={{ color: '#ff4d4f' }} />
                            </Popconfirm>
                          </Tooltip>,
                        ]
                      : []),
                  ]}
                >
                  <div style={{ marginBottom: 8, display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                    <Tag color={CATEGORY_COLORS[template.category] ?? 'default'}>
                      {template.category}
                    </Tag>
                    {template.is_public ? (
                      <Tooltip title={t('prompts.public')}>
                        <GlobalOutlined style={{ color: '#52c41a' }} />
                      </Tooltip>
                    ) : (
                      <Tooltip title={t('prompts.private')}>
                        <LockOutlined style={{ color: '#8c8c8c' }} />
                      </Tooltip>
                    )}
                  </div>

                  <Card.Meta
                    title={
                      <Text strong style={{ fontSize: 14 }}>
                        {template.name}
                      </Text>
                    }
                    description={
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {template.description || t('prompts.noDescription')}
                      </Text>
                    }
                  />

                  <div style={{ marginTop: 12 }}>
                    {template.variables.slice(0, 3).map((v) => (
                      <Badge
                        key={v}
                        count={`{${v}}`}
                        style={{
                          backgroundColor: '#f0f0f0',
                          color: '#595959',
                          marginRight: 4,
                          marginBottom: 4,
                          fontSize: 11,
                        }}
                      />
                    ))}
                    {template.variables.length > 3 && (
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        +{template.variables.length - 3} more
                      </Text>
                    )}
                  </div>

                  <div style={{ marginTop: 12 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                      <Rate
                        allowHalf
                        value={template.rating_avg}
                        onChange={(val) => handleRate(template.id, Math.round(val))}
                        style={{ fontSize: 14 }}
                      />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        ({template.rating_count})
                      </Text>
                    </div>
                    {template.author_name && (
                      <Text type="secondary" style={{ fontSize: 11, marginTop: 4, display: 'block' }}>
                        by {template.author_name} · v{template.version}
                      </Text>
                    )}
                  </div>
                </Card>
              </Col>
            ))}
          </Row>
        )}
      </Spin>

      {/* View Modal */}
      <Modal
        title={selected?.name}
        open={viewOpen}
        onCancel={() => setViewOpen(false)}
        footer={[
          <Button key="copy" icon={<CopyOutlined />} onClick={() => selected && handleCopyContent(selected.content)}>
            {t('prompts.copyContent')}
          </Button>,
          <Button key="fork" icon={<ForkOutlined />} onClick={() => selected && handleFork(selected.id)}>
            {t('prompts.fork')}
          </Button>,
          <Button key="close" onClick={() => setViewOpen(false)}>
            {t('action.close')}
          </Button>,
        ]}
        width={700}
      >
        {selected && (
          <div>
            <Space wrap style={{ marginBottom: 12 }}>
              <Tag color={CATEGORY_COLORS[selected.category] ?? 'default'}>{selected.category}</Tag>
              {selected.is_public ? (
                <Tag icon={<GlobalOutlined />} color="green">{t('prompts.public')}</Tag>
              ) : (
                <Tag icon={<LockOutlined />}>{t('prompts.private')}</Tag>
              )}
              <Tag>v{selected.version}</Tag>
            </Space>

            {selected.description && (
              <Paragraph type="secondary">{selected.description}</Paragraph>
            )}

            {selected.author_name && (
              <Text type="secondary" style={{ display: 'block', marginBottom: 12 }}>
                {t('prompts.author')}: {selected.author_name}
              </Text>
            )}

            {selected.variables.length > 0 && (
              <>
                <Divider orientation="left">{t('prompts.variables')}</Divider>
                <Space wrap style={{ marginBottom: 12 }}>
                  {selected.variables.map((v) => (
                    <Tag key={v} color="blue">{`{${v}}`}</Tag>
                  ))}
                </Space>
              </>
            )}

            <Divider orientation="left">{t('prompts.content')}</Divider>
            <pre
              style={{
                background: '#f5f5f5',
                padding: 16,
                borderRadius: 8,
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
                maxHeight: 400,
                overflowY: 'auto',
                fontSize: 13,
              }}
            >
              {selected.content}
            </pre>

            <div style={{ marginTop: 12, display: 'flex', alignItems: 'center', gap: 8 }}>
              <Rate
                allowHalf
                value={selected.rating_avg}
                onChange={(val) => handleRate(selected.id, Math.round(val))}
              />
              <Text type="secondary">
                {selected.rating_avg.toFixed(1)} ({selected.rating_count} {t('prompts.ratings')})
              </Text>
            </div>
          </div>
        )}
      </Modal>

      {/* Create Modal */}
      <Modal
        title={t('prompts.createTitle')}
        open={createOpen}
        onCancel={() => {
          setCreateOpen(false);
          createForm.resetFields();
          setVariables([]);
          setVariableInput('');
        }}
        footer={null}
        width={700}
      >
        <Form form={createForm} layout="vertical" onFinish={handleCreate}>
          <Form.Item name="name" label={t('prompts.name')} rules={[{ required: true }]}>
            <Input placeholder={t('prompts.namePlaceholder')} />
          </Form.Item>
          <Form.Item name="description" label={t('prompts.description')}>
            <Input placeholder={t('prompts.descriptionPlaceholder')} />
          </Form.Item>
          <Row gutter={16}>
            <Col span={12}>
              <Form.Item name="category" label={t('prompts.category')} initialValue="general">
                <Select>
                  {CATEGORIES.map((cat) => (
                    <Option key={cat} value={cat}>{cat}</Option>
                  ))}
                </Select>
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item name="is_public" label={t('prompts.visibility')} initialValue={true}>
                <Select>
                  <Option value={true}>{t('prompts.public')}</Option>
                  <Option value={false}>{t('prompts.private')}</Option>
                </Select>
              </Form.Item>
            </Col>
          </Row>
          <Form.Item label={t('prompts.variables')}>
            <div style={{ marginBottom: 8 }}>
              <Space.Compact style={{ width: '100%' }}>
                <Input
                  value={variableInput}
                  onChange={(e) => setVariableInput(e.target.value)}
                  onPressEnter={addVariable}
                  placeholder={t('prompts.variablePlaceholder')}
                />
                <Button onClick={addVariable}>{t('prompts.addVariable')}</Button>
              </Space.Compact>
            </div>
            <Space wrap>
              {variables.map((v) => (
                <Tag
                  key={v}
                  closable
                  onClose={() => setVariables(variables.filter((x) => x !== v))}
                  color="blue"
                >
                  {`{${v}}`}
                </Tag>
              ))}
            </Space>
          </Form.Item>
          <Form.Item
            name="content"
            label={t('prompts.content')}
            rules={[{ required: true }]}
          >
            <TextArea
              rows={8}
              placeholder={t('prompts.contentPlaceholder')}
              style={{ fontFamily: 'monospace' }}
            />
          </Form.Item>
          <Form.Item style={{ textAlign: 'right', marginBottom: 0 }}>
            <Space>
              <Button onClick={() => setCreateOpen(false)}>{t('action.cancel')}</Button>
              <Button type="primary" htmlType="submit" loading={submitting}>
                {t('action.create')}
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>

      {/* Edit Modal */}
      <Modal
        title={t('prompts.editTitle')}
        open={editOpen}
        onCancel={() => {
          setEditOpen(false);
          setSelected(null);
          editForm.resetFields();
        }}
        footer={null}
        width={700}
      >
        <Form form={editForm} layout="vertical" onFinish={handleUpdate}>
          <Form.Item name="name" label={t('prompts.name')} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label={t('prompts.description')}>
            <Input />
          </Form.Item>
          <Row gutter={16}>
            <Col span={12}>
              <Form.Item name="category" label={t('prompts.category')}>
                <Select>
                  {CATEGORIES.map((cat) => (
                    <Option key={cat} value={cat}>{cat}</Option>
                  ))}
                </Select>
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item name="is_public" label={t('prompts.visibility')}>
                <Select>
                  <Option value={true}>{t('prompts.public')}</Option>
                  <Option value={false}>{t('prompts.private')}</Option>
                </Select>
              </Form.Item>
            </Col>
          </Row>
          <Form.Item label={t('prompts.variables')}>
            <div style={{ marginBottom: 8 }}>
              <Space.Compact style={{ width: '100%' }}>
                <Input
                  value={editVariableInput}
                  onChange={(e) => setEditVariableInput(e.target.value)}
                  onPressEnter={addEditVariable}
                  placeholder={t('prompts.variablePlaceholder')}
                />
                <Button onClick={addEditVariable}>{t('prompts.addVariable')}</Button>
              </Space.Compact>
            </div>
            <Space wrap>
              {editVariables.map((v) => (
                <Tag
                  key={v}
                  closable
                  onClose={() => setEditVariables(editVariables.filter((x) => x !== v))}
                  color="blue"
                >
                  {`{${v}}`}
                </Tag>
              ))}
            </Space>
          </Form.Item>
          <Form.Item
            name="content"
            label={t('prompts.content')}
            rules={[{ required: true }]}
          >
            <TextArea rows={8} style={{ fontFamily: 'monospace' }} />
          </Form.Item>
          <Form.Item style={{ textAlign: 'right', marginBottom: 0 }}>
            <Space>
              <Button onClick={() => setEditOpen(false)}>{t('action.cancel')}</Button>
              <Button type="primary" htmlType="submit" loading={submitting}>
                {t('action.save')}
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
