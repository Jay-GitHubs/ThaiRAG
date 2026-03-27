import { useState, useEffect, useCallback } from 'react';
import {
  Button,
  Collapse,
  Form,
  Input,
  List,
  Popconfirm,
  Select,
  Space,
  Spin,
  Tabs,
  Tag,
  Typography,
  message,
} from 'antd';
import { PlusOutlined, DeleteOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useI18n } from '../../i18n';
import type {
  DocumentComment,
  DocumentAnnotation,
  DocumentReview,
} from '../../api/collaboration';
import {
  listComments,
  createComment,
  deleteComment,
  listAnnotations,
  createAnnotation,
  deleteAnnotation,
  listReviews,
  createReview,
  updateReviewStatus,
} from '../../api/collaboration';

const REVIEW_STATUSES = ['pending', 'approved', 'rejected', 'changes_requested'];

const REVIEW_STATUS_COLORS: Record<string, string> = {
  pending: 'orange',
  approved: 'green',
  rejected: 'red',
  changes_requested: 'blue',
};

// ── Comments Tab ──────────────────────────────────────────────────────

function CommentsTab({ wsId, docId }: { wsId: string; docId: string }) {
  const { t } = useI18n();
  const [comments, setComments] = useState<DocumentComment[]>([]);
  const [loading, setLoading] = useState(false);
  const [text, setText] = useState('');
  const [submitting, setSubmitting] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listComments(wsId, docId);
      setComments(data);
    } catch {
      message.error(t('collaboration.loadFailed'));
    } finally {
      setLoading(false);
    }
  }, [wsId, docId, t]);

  useEffect(() => {
    load();
  }, [load]);

  const handleSubmit = async () => {
    if (!text.trim()) return;
    setSubmitting(true);
    try {
      await createComment(wsId, docId, text.trim());
      setText('');
      load();
    } catch {
      message.error(t('collaboration.createFailed'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (commentId: string) => {
    try {
      await deleteComment(wsId, docId, commentId);
      load();
    } catch {
      message.error(t('collaboration.deleteFailed'));
    }
  };

  // Build thread tree: top-level comments first, replies indented
  const topLevel = comments.filter((c) => !c.parent_id);
  const replies = (parentId: string) => comments.filter((c) => c.parent_id === parentId);

  const renderComment = (c: DocumentComment, indent = 0) => (
    <div key={c.id} style={{ marginLeft: indent * 24, marginBottom: 8 }}>
      <div
        style={{
          background: 'rgba(0,0,0,0.03)',
          borderRadius: 6,
          padding: '8px 12px',
          position: 'relative',
        }}
      >
        <Space style={{ marginBottom: 4 }}>
          <Typography.Text strong>{c.user_name ?? c.user_id}</Typography.Text>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {dayjs(c.created_at).format('YYYY-MM-DD HH:mm')}
          </Typography.Text>
        </Space>
        <div>{c.text}</div>
        <Popconfirm
          title={t('collaboration.deleteComment')}
          onConfirm={() => handleDelete(c.id)}
        >
          <Button
            size="small"
            type="text"
            danger
            icon={<DeleteOutlined />}
            style={{ position: 'absolute', top: 8, right: 8 }}
          />
        </Popconfirm>
      </div>
      {replies(c.id).map((r) => renderComment(r, 1))}
    </div>
  );

  return (
    <Spin spinning={loading}>
      <div style={{ marginBottom: 16 }}>
        {topLevel.length === 0 && !loading ? (
          <Typography.Text type="secondary">{t('collaboration.noComments')}</Typography.Text>
        ) : (
          topLevel.map((c) => renderComment(c))
        )}
      </div>
      <Space.Compact style={{ width: '100%' }}>
        <Input.TextArea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={t('collaboration.commentPlaceholder')}
          rows={2}
          style={{ flex: 1 }}
        />
      </Space.Compact>
      <Button
        type="primary"
        icon={<PlusOutlined />}
        onClick={handleSubmit}
        loading={submitting}
        disabled={!text.trim()}
        style={{ marginTop: 8 }}
      >
        {t('collaboration.addComment')}
      </Button>
    </Spin>
  );
}

// ── Annotations Tab ───────────────────────────────────────────────────

function AnnotationsTab({ wsId, docId }: { wsId: string; docId: string }) {
  const { t } = useI18n();
  const [annotations, setAnnotations] = useState<DocumentAnnotation[]>([]);
  const [loading, setLoading] = useState(false);
  const [form] = Form.useForm();
  const [submitting, setSubmitting] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listAnnotations(wsId, docId);
      setAnnotations(data);
    } catch {
      message.error(t('collaboration.loadFailed'));
    } finally {
      setLoading(false);
    }
  }, [wsId, docId, t]);

  useEffect(() => {
    load();
  }, [load]);

  const handleSubmit = async (values: { text: string; chunk_id?: string }) => {
    setSubmitting(true);
    try {
      await createAnnotation(wsId, docId, { text: values.text, chunk_id: values.chunk_id });
      form.resetFields();
      load();
    } catch {
      message.error(t('collaboration.createFailed'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (annotationId: string) => {
    try {
      await deleteAnnotation(wsId, docId, annotationId);
      load();
    } catch {
      message.error(t('collaboration.deleteFailed'));
    }
  };

  return (
    <Spin spinning={loading}>
      <List
        dataSource={annotations}
        locale={{ emptyText: t('collaboration.noAnnotations') }}
        renderItem={(a) => (
          <List.Item
            actions={[
              <Popconfirm
                key="del"
                title={t('collaboration.deleteAnnotation')}
                onConfirm={() => handleDelete(a.id)}
              >
                <Button size="small" danger icon={<DeleteOutlined />} />
              </Popconfirm>,
            ]}
          >
            <List.Item.Meta
              title={
                <Space>
                  <Typography.Text strong>{a.user_name ?? a.user_id}</Typography.Text>
                  {a.chunk_id && <Tag>{a.chunk_id}</Tag>}
                  <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                    {dayjs(a.created_at).format('YYYY-MM-DD HH:mm')}
                  </Typography.Text>
                </Space>
              }
              description={a.text}
            />
          </List.Item>
        )}
        style={{ marginBottom: 16 }}
      />
      <Form form={form} layout="vertical" onFinish={handleSubmit}>
        <Form.Item name="chunk_id" label={t('collaboration.chunkId')}>
          <Input placeholder={t('collaboration.chunkIdPlaceholder')} />
        </Form.Item>
        <Form.Item
          name="text"
          label={t('collaboration.annotationText')}
          rules={[{ required: true }]}
        >
          <Input.TextArea rows={2} placeholder={t('collaboration.annotationPlaceholder')} />
        </Form.Item>
        <Button
          type="primary"
          htmlType="submit"
          icon={<PlusOutlined />}
          loading={submitting}
        >
          {t('collaboration.addAnnotation')}
        </Button>
      </Form>
    </Spin>
  );
}

// ── Reviews Tab ───────────────────────────────────────────────────────

function ReviewsTab({ wsId, docId }: { wsId: string; docId: string }) {
  const { t } = useI18n();
  const [reviews, setReviews] = useState<DocumentReview[]>([]);
  const [loading, setLoading] = useState(false);
  const [form] = Form.useForm();
  const [submitting, setSubmitting] = useState(false);
  const [updatingId, setUpdatingId] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listReviews(wsId, docId);
      setReviews(data);
    } catch {
      message.error(t('collaboration.loadFailed'));
    } finally {
      setLoading(false);
    }
  }, [wsId, docId, t]);

  useEffect(() => {
    load();
  }, [load]);

  const handleCreate = async (values: { status: string; comments?: string }) => {
    setSubmitting(true);
    try {
      await createReview(wsId, docId, values.status, values.comments);
      form.resetFields();
      load();
    } catch {
      message.error(t('collaboration.createFailed'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleStatusUpdate = async (review: DocumentReview, status: string) => {
    setUpdatingId(review.id);
    try {
      await updateReviewStatus(wsId, docId, review.id, status, review.comments);
      load();
    } catch {
      message.error(t('collaboration.updateFailed'));
    } finally {
      setUpdatingId(null);
    }
  };

  return (
    <Spin spinning={loading}>
      <List
        dataSource={reviews}
        locale={{ emptyText: t('collaboration.noReviews') }}
        renderItem={(r) => (
          <List.Item
            actions={[
              <Select
                key="status"
                size="small"
                value={r.status}
                style={{ width: 160 }}
                loading={updatingId === r.id}
                onChange={(s) => handleStatusUpdate(r, s)}
                options={REVIEW_STATUSES.map((s) => ({ label: s.replace('_', ' '), value: s }))}
              />,
            ]}
          >
            <List.Item.Meta
              title={
                <Space>
                  <Typography.Text strong>{r.reviewer_name ?? r.reviewer_id}</Typography.Text>
                  <Tag color={REVIEW_STATUS_COLORS[r.status] ?? 'default'}>
                    {r.status.replace('_', ' ')}
                  </Tag>
                  <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                    {dayjs(r.created_at).format('YYYY-MM-DD HH:mm')}
                  </Typography.Text>
                </Space>
              }
              description={r.comments}
            />
          </List.Item>
        )}
        style={{ marginBottom: 16 }}
      />
      <Form form={form} layout="vertical" onFinish={handleCreate}>
        <Form.Item name="status" label={t('collaboration.reviewStatus')} initialValue="pending">
          <Select
            options={REVIEW_STATUSES.map((s) => ({ label: s.replace('_', ' '), value: s }))}
          />
        </Form.Item>
        <Form.Item name="comments" label={t('collaboration.reviewComments')}>
          <Input.TextArea rows={2} placeholder={t('collaboration.reviewCommentsPlaceholder')} />
        </Form.Item>
        <Button type="primary" htmlType="submit" icon={<PlusOutlined />} loading={submitting}>
          {t('collaboration.createReview')}
        </Button>
      </Form>
    </Spin>
  );
}

// ── Main CollaborationPanel ───────────────────────────────────────────

interface CollaborationPanelProps {
  wsId: string;
  docId: string;
  docTitle?: string;
}

export function CollaborationPanel({ wsId, docId, docTitle }: CollaborationPanelProps) {
  const { t } = useI18n();

  const tabs = [
    {
      key: 'comments',
      label: t('collaboration.comments'),
      children: <CommentsTab wsId={wsId} docId={docId} />,
    },
    {
      key: 'annotations',
      label: t('collaboration.annotations'),
      children: <AnnotationsTab wsId={wsId} docId={docId} />,
    },
    {
      key: 'reviews',
      label: t('collaboration.reviews'),
      children: <ReviewsTab wsId={wsId} docId={docId} />,
    },
  ];

  return (
    <Collapse
      items={[
        {
          key: 'collab',
          label: (
            <Typography.Text strong>
              {t('collaboration.panelTitle')}
              {docTitle ? ` — ${docTitle}` : ''}
            </Typography.Text>
          ),
          children: <Tabs items={tabs} size="small" />,
        },
      ]}
    />
  );
}
