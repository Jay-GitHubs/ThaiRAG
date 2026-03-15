import { useState } from 'react';
import {
  Typography,
  Tabs,
  Card,
  Row,
  Col,
  Statistic,
  Table,
  Tag,
  Button,
  Space,
  Tooltip,
  Popconfirm,
  InputNumber,
  Slider,
  Alert,
  Empty,
  Spin,
  message,
  theme,
} from 'antd';
import {
  LikeOutlined,
  DislikeOutlined,
  TrophyOutlined,
  ThunderboltOutlined,
  QuestionCircleOutlined,
  DeleteOutlined,
  CheckOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getFeedbackStats,
  listFeedbackEntries,
  getDocumentBoosts,
  listGoldenExamples,
  deleteGoldenExample,
  getRetrievalParams,
  updateRetrievalParams,
} from '../api/settings';
import type { FeedbackEntry, DocumentBoost, GoldenExample } from '../api/types';

export function FeedbackPage() {
  return (
    <>
      <Space align="baseline" style={{ marginBottom: 16 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>
          Feedback & Auto-Tuning
        </Typography.Title>
        <Tooltip title="User feedback from Test Chat is collected here. The system uses this data to automatically improve answer quality: document relevance scoring, quality thresholds, and retrieval parameters.">
          <QuestionCircleOutlined style={{ fontSize: 16 }} />
        </Tooltip>
      </Space>

      <Tabs
        defaultActiveKey="overview"
        items={[
          { key: 'overview', label: 'Overview', children: <OverviewTab /> },
          { key: 'entries', label: 'Feedback Log', children: <EntriesTab /> },
          { key: 'documents', label: 'Document Scores', children: <DocumentBoostsTab /> },
          { key: 'golden', label: 'Golden Examples', children: <GoldenExamplesTab /> },
          { key: 'tuning', label: 'Retrieval Tuning', children: <RetrievalTuningTab /> },
        ]}
      />
    </>
  );
}

// ── Overview Tab ────────────────────────────────────────────────────

function OverviewTab() {
  const { token: themeToken } = theme.useToken();
  const stats = useQuery({ queryKey: ['feedback-stats'], queryFn: getFeedbackStats, refetchInterval: 30_000 });

  if (stats.isLoading) return <Spin />;
  if (!stats.data) return <Empty description="No feedback data" />;

  const { total, positive, negative, positive_rate, current_threshold, adaptive_threshold, adaptive_enabled } = stats.data;
  const satisfactionColor = positive_rate >= 0.8 ? '#52c41a' : positive_rate >= 0.5 ? '#faad14' : '#cf1322';

  return (
    <>
      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic title="Total Feedback" value={total} prefix={<ThunderboltOutlined />} />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Positive"
              value={positive}
              prefix={<LikeOutlined />}
              valueStyle={{ color: '#52c41a' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="Negative"
              value={negative}
              prefix={<DislikeOutlined />}
              valueStyle={{ color: '#cf1322' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title={
                <Tooltip title="Percentage of positive feedback. Higher is better. Below 50% indicates quality issues.">
                  <span>Satisfaction Rate <QuestionCircleOutlined /></span>
                </Tooltip>
              }
              value={`${(positive_rate * 100).toFixed(1)}%`}
              valueStyle={{ color: satisfactionColor }}
            />
          </Card>
        </Col>
      </Row>

      <Card style={{ marginTop: 16 }} title="Quality Guard Threshold">
        <Row gutter={[16, 16]}>
          <Col xs={12} sm={8}>
            <Statistic
              title="Current Threshold"
              value={current_threshold.toFixed(2)}
            />
          </Col>
          {adaptive_enabled && adaptive_threshold != null && (
            <Col xs={12} sm={8}>
              <Statistic
                title={
                  <Tooltip title="Threshold computed from feedback data. Updates automatically as more feedback is collected.">
                    <span>Adaptive Threshold <QuestionCircleOutlined /></span>
                  </Tooltip>
                }
                value={adaptive_threshold.toFixed(2)}
                valueStyle={{ color: themeToken.colorPrimary }}
              />
            </Col>
          )}
          <Col xs={24}>
            <Tag color={adaptive_enabled ? 'green' : 'default'}>
              Adaptive Tuning: {adaptive_enabled ? 'Enabled' : 'Disabled'}
            </Tag>
            <Typography.Text type="secondary" style={{ marginLeft: 8 }}>
              {adaptive_enabled
                ? `Automatically adjusts quality threshold based on ${stats.data.min_samples}+ feedback samples.`
                : 'Enable in Chat Pipeline settings to auto-adjust threshold from feedback.'}
            </Typography.Text>
          </Col>
        </Row>
      </Card>

      {total === 0 && (
        <Alert
          style={{ marginTop: 16 }}
          type="info"
          message="No feedback collected yet"
          description="Use the Test Chat page to ask questions and rate the answers with thumbs up/down. The system will start auto-tuning after collecting enough feedback."
        />
      )}
    </>
  );
}

// ── Entries Tab ─────────────────────────────────────────────────────

function EntriesTab() {
  const [filter, setFilter] = useState<string>('all');
  const [page, setPage] = useState(1);
  const pageSize = 20;

  const entries = useQuery({
    queryKey: ['feedback-entries', filter, page],
    queryFn: () => listFeedbackEntries({ limit: pageSize, offset: (page - 1) * pageSize, filter }),
  });

  const columns = [
    {
      title: 'Rating',
      dataIndex: 'thumbs_up',
      width: 80,
      render: (v: boolean) => v
        ? <Tag color="green" icon={<LikeOutlined />}>Good</Tag>
        : <Tag color="red" icon={<DislikeOutlined />}>Bad</Tag>,
    },
    {
      title: 'Query',
      dataIndex: 'query',
      ellipsis: true,
      render: (v: string | undefined) => v || <Typography.Text type="secondary">-</Typography.Text>,
    },
    {
      title: 'Comment',
      dataIndex: 'comment',
      ellipsis: true,
      render: (v: string | undefined) => v || <Typography.Text type="secondary">-</Typography.Text>,
    },
    {
      title: 'Chunks',
      dataIndex: 'doc_ids',
      width: 80,
      render: (ids: string[]) => ids?.length ?? 0,
    },
    {
      title: 'Time',
      dataIndex: 'timestamp',
      width: 160,
      render: (v: number) => new Date(v * 1000).toLocaleString(),
    },
  ];

  return (
    <>
      <Space style={{ marginBottom: 16 }}>
        <Button
          type={filter === 'all' ? 'primary' : 'default'}
          onClick={() => { setFilter('all'); setPage(1); }}
        >
          All
        </Button>
        <Button
          type={filter === 'positive' ? 'primary' : 'default'}
          onClick={() => { setFilter('positive'); setPage(1); }}
          icon={<LikeOutlined />}
        >
          Positive
        </Button>
        <Button
          type={filter === 'negative' ? 'primary' : 'default'}
          onClick={() => { setFilter('negative'); setPage(1); }}
          icon={<DislikeOutlined />}
        >
          Negative
        </Button>
      </Space>

      <Table<FeedbackEntry>
        dataSource={entries.data?.entries ?? []}
        columns={columns}
        rowKey="response_id"
        loading={entries.isLoading}
        pagination={{
          current: page,
          pageSize,
          total: entries.data?.total_filtered ?? 0,
          onChange: setPage,
          showTotal: (t) => `${t} entries`,
        }}
        expandable={{
          expandedRowRender: (record) => (
            <div style={{ padding: 8 }}>
              {record.answer && (
                <div style={{ marginBottom: 8 }}>
                  <Typography.Text strong>Answer:</Typography.Text>
                  <div style={{ whiteSpace: 'pre-wrap', maxHeight: 200, overflowY: 'auto', fontSize: 13, marginTop: 4 }}>
                    {record.answer}
                  </div>
                </div>
              )}
              {record.chunk_scores.length > 0 && (
                <Space wrap>
                  <Typography.Text strong>Chunk scores:</Typography.Text>
                  {record.chunk_scores.map((s, i) => (
                    <Tag key={i}>{s.toFixed(4)}</Tag>
                  ))}
                </Space>
              )}
            </div>
          ),
        }}
      />
    </>
  );
}

// ── Document Boosts Tab ─────────────────────────────────────────────

function DocumentBoostsTab() {
  const boosts = useQuery({ queryKey: ['document-boosts'], queryFn: getDocumentBoosts });

  const columns = [
    {
      title: 'Document ID',
      dataIndex: 'doc_id',
      ellipsis: true,
    },
    {
      title: 'Boost',
      dataIndex: 'boost',
      width: 120,
      sorter: (a: DocumentBoost, b: DocumentBoost) => a.boost - b.boost,
      render: (v: number) => {
        const color = v > 1.1 ? 'green' : v < 0.9 ? 'red' : 'default';
        const label = v > 1.0 ? `+${((v - 1) * 100).toFixed(0)}%` : v < 1.0 ? `${((v - 1) * 100).toFixed(0)}%` : 'Neutral';
        return <Tag color={color}>{label} ({v.toFixed(2)})</Tag>;
      },
    },
    {
      title: (
        <Tooltip title="Number of positive feedback entries involving this document">
          <span><LikeOutlined /> Positive</span>
        </Tooltip>
      ),
      dataIndex: 'positive_count',
      width: 100,
      sorter: (a: DocumentBoost, b: DocumentBoost) => a.positive_count - b.positive_count,
    },
    {
      title: (
        <Tooltip title="Number of negative feedback entries involving this document">
          <span><DislikeOutlined /> Negative</span>
        </Tooltip>
      ),
      dataIndex: 'negative_count',
      width: 100,
      sorter: (a: DocumentBoost, b: DocumentBoost) => a.negative_count - b.negative_count,
    },
    {
      title: 'Total',
      dataIndex: 'total_count',
      width: 80,
      sorter: (a: DocumentBoost, b: DocumentBoost) => a.total_count - b.total_count,
    },
  ];

  return (
    <>
      <Alert
        type="info"
        style={{ marginBottom: 16 }}
        message="Document Quality Scores"
        description="Each document gets a boost/penalty based on user feedback. Documents with consistently negative feedback are de-prioritized in search results. Minimum 3 feedback entries needed before adjustment kicks in."
      />
      <Table<DocumentBoost>
        dataSource={boosts.data ?? []}
        columns={columns}
        rowKey="doc_id"
        loading={boosts.isLoading}
        pagination={{ pageSize: 20 }}
        locale={{ emptyText: 'No document feedback data yet. Rate answers in Test Chat to build document scores.' }}
      />
    </>
  );
}

// ── Golden Examples Tab ─────────────────────────────────────────────

function GoldenExamplesTab() {
  const queryClient = useQueryClient();
  const examples = useQuery({ queryKey: ['golden-examples'], queryFn: listGoldenExamples });
  const deleteMutation = useMutation({
    mutationFn: deleteGoldenExample,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['golden-examples'] });
      message.success('Golden example deleted');
    },
  });

  const columns = [
    {
      title: 'Query',
      dataIndex: 'query',
      ellipsis: true,
    },
    {
      title: 'Answer',
      dataIndex: 'answer',
      ellipsis: true,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      width: 160,
      render: (v: number) => new Date(v * 1000).toLocaleString(),
    },
    {
      title: 'Actions',
      width: 80,
      render: (_: unknown, record: GoldenExample) => (
        <Popconfirm
          title="Delete this golden example?"
          onConfirm={() => deleteMutation.mutate(record.id)}
        >
          <Button danger size="small" icon={<DeleteOutlined />} />
        </Popconfirm>
      ),
    },
  ];

  return (
    <>
      <Alert
        type="info"
        style={{ marginBottom: 16 }}
        message="Few-Shot Golden Examples"
        description="Highly-rated Q&A pairs saved as golden examples. These are injected into the system prompt as few-shot demonstrations, teaching the LLM the style and quality of good answers. Max 5 examples are used per prompt."
        icon={<TrophyOutlined />}
      />
      <Table<GoldenExample>
        dataSource={examples.data ?? []}
        columns={columns}
        rowKey="id"
        loading={examples.isLoading}
        pagination={{ pageSize: 10 }}
        expandable={{
          expandedRowRender: (record) => (
            <div style={{ padding: 8 }}>
              <Typography.Text strong>Full Answer:</Typography.Text>
              <div style={{ whiteSpace: 'pre-wrap', marginTop: 4, fontSize: 13 }}>
                {record.answer}
              </div>
            </div>
          ),
        }}
        locale={{ emptyText: 'No golden examples yet. Use the star button in Test Chat to save great Q&A pairs.' }}
      />
    </>
  );
}

// ── Retrieval Tuning Tab ────────────────────────────────────────────

function RetrievalTuningTab() {
  const { token: themeToken } = theme.useToken();
  const queryClient = useQueryClient();
  const params = useQuery({ queryKey: ['retrieval-params'], queryFn: getRetrievalParams });
  const updateMutation = useMutation({
    mutationFn: updateRetrievalParams,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['retrieval-params'] });
      message.success('Retrieval parameters updated');
    },
  });

  const [localTopK, setLocalTopK] = useState<number | null>(null);
  const [localVectorW, setLocalVectorW] = useState<number | null>(null);
  const [localBm25W, setLocalBm25W] = useState<number | null>(null);
  const [localMinScore, setLocalMinScore] = useState<number | null>(null);

  if (params.isLoading) return <Spin />;
  if (!params.data) return <Empty />;

  const data = params.data;
  const topK = localTopK ?? data.top_k;
  const vectorW = localVectorW ?? data.vector_weight;
  const bm25W = localBm25W ?? data.bm25_weight;
  const minScore = localMinScore ?? data.min_score_threshold;

  const hasChanges =
    localTopK !== null || localVectorW !== null || localBm25W !== null || localMinScore !== null;

  return (
    <>
      {data.suggested && (
        <Alert
          type="warning"
          style={{ marginBottom: 16 }}
          icon={<SettingOutlined />}
          message="Auto-Tuning Suggestion Available"
          description={
            <div>
              <Typography.Paragraph style={{ marginBottom: 8 }}>
                {data.suggested.reason}
              </Typography.Paragraph>
              <Space>
                <Tag>top_k: {data.suggested.top_k}</Tag>
                <Tag>vector_weight: {data.suggested.vector_weight}</Tag>
                <Tag>bm25_weight: {data.suggested.bm25_weight}</Tag>
                <Button
                  type="primary"
                  size="small"
                  icon={<CheckOutlined />}
                  loading={updateMutation.isPending}
                  onClick={() => updateMutation.mutate({ apply_suggestions: true })}
                >
                  Apply Suggestions
                </Button>
              </Space>
            </div>
          }
        />
      )}

      {data.auto_tuned && (
        <Alert
          type="success"
          style={{ marginBottom: 16 }}
          message={`Parameters auto-tuned from ${data.samples_used} feedback samples`}
        />
      )}

      <Card title="Retrieval Parameters">
        <Row gutter={[24, 24]}>
          <Col xs={24} sm={12}>
            <Typography.Text strong>Top K (chunks to retrieve)</Typography.Text>
            <Tooltip title="Number of document chunks retrieved per query. More chunks = more context but higher token costs.">
              <QuestionCircleOutlined style={{ marginLeft: 6, color: themeToken.colorTextSecondary }} />
            </Tooltip>
            <div style={{ marginTop: 8 }}>
              <InputNumber
                min={1}
                max={20}
                value={topK}
                onChange={(v) => setLocalTopK(v)}
                style={{ width: '100%' }}
              />
            </div>
          </Col>

          <Col xs={24} sm={12}>
            <Typography.Text strong>Min Score Threshold</Typography.Text>
            <Tooltip title="Minimum relevance score for a chunk to be included. 0 = include all. Higher values filter out low-relevance chunks.">
              <QuestionCircleOutlined style={{ marginLeft: 6, color: themeToken.colorTextSecondary }} />
            </Tooltip>
            <div style={{ marginTop: 8 }}>
              <Slider
                min={0}
                max={0.1}
                step={0.001}
                value={minScore}
                onChange={(v) => setLocalMinScore(v)}
              />
              <Typography.Text type="secondary">{minScore.toFixed(3)}</Typography.Text>
            </div>
          </Col>

          <Col xs={24} sm={12}>
            <Typography.Text strong>Vector Search Weight</Typography.Text>
            <Tooltip title="Weight for semantic (vector) search in RRF fusion. Higher = more emphasis on meaning similarity.">
              <QuestionCircleOutlined style={{ marginLeft: 6, color: themeToken.colorTextSecondary }} />
            </Tooltip>
            <div style={{ marginTop: 8 }}>
              <Slider
                min={0}
                max={3}
                step={0.1}
                value={vectorW}
                onChange={(v) => setLocalVectorW(v)}
              />
              <Typography.Text type="secondary">{vectorW.toFixed(1)}</Typography.Text>
            </div>
          </Col>

          <Col xs={24} sm={12}>
            <Typography.Text strong>BM25 Search Weight</Typography.Text>
            <Tooltip title="Weight for keyword (BM25) search in RRF fusion. Higher = more emphasis on exact keyword matching.">
              <QuestionCircleOutlined style={{ marginLeft: 6, color: themeToken.colorTextSecondary }} />
            </Tooltip>
            <div style={{ marginTop: 8 }}>
              <Slider
                min={0}
                max={3}
                step={0.1}
                value={bm25W}
                onChange={(v) => setLocalBm25W(v)}
              />
              <Typography.Text type="secondary">{bm25W.toFixed(1)}</Typography.Text>
            </div>
          </Col>
        </Row>

        <div style={{ marginTop: 16, display: 'flex', gap: 8 }}>
          <Button
            type="primary"
            disabled={!hasChanges}
            loading={updateMutation.isPending}
            onClick={() => {
              updateMutation.mutate({
                top_k: localTopK ?? undefined,
                vector_weight: localVectorW ?? undefined,
                bm25_weight: localBm25W ?? undefined,
                min_score_threshold: localMinScore ?? undefined,
              });
              setLocalTopK(null);
              setLocalVectorW(null);
              setLocalBm25W(null);
              setLocalMinScore(null);
            }}
          >
            Save Changes
          </Button>
          {hasChanges && (
            <Button
              onClick={() => {
                setLocalTopK(null);
                setLocalVectorW(null);
                setLocalBm25W(null);
                setLocalMinScore(null);
              }}
            >
              Reset
            </Button>
          )}
        </div>
      </Card>
    </>
  );
}
