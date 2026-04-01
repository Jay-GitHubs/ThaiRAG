import { useState } from 'react';
import {
  Typography,
  Card,
  Table,
  Space,
  Tabs,
  Input,
  Button,
  Tag,
  Empty,
  Spin,
  message,
  Tour,
} from 'antd';
import {
  ApartmentOutlined,
  SearchOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
} from '@ant-design/icons';
import { getLineageByResponse, getLineageByDocument } from '../api/lineage';
import type { LineageRecord } from '../api/lineage';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getLineageSteps } from '../tours/steps/lineage';

const lineageColumns = [
  {
    title: 'Query',
    dataIndex: 'query',
    ellipsis: true,
  },
  {
    title: 'Chunk ID',
    dataIndex: 'chunk_id',
    width: 200,
    ellipsis: true,
    render: (v: string) => (
      <Typography.Text code style={{ fontSize: 12 }}>
        {v}
      </Typography.Text>
    ),
  },
  {
    title: 'Doc Title',
    dataIndex: 'doc_title',
    width: 220,
    ellipsis: true,
  },
  {
    title: 'Score',
    dataIndex: 'score',
    width: 90,
    render: (v: number) => (
      <span
        style={{
          color: v >= 0.8 ? '#52c41a' : v >= 0.5 ? '#faad14' : '#cf1322',
        }}
      >
        {v.toFixed(4)}
      </span>
    ),
    sorter: (a: LineageRecord, b: LineageRecord) => a.score - b.score,
  },
  {
    title: 'Rank',
    dataIndex: 'rank',
    width: 70,
    sorter: (a: LineageRecord, b: LineageRecord) => a.rank - b.rank,
  },
  {
    title: 'Contributed',
    dataIndex: 'contributed',
    width: 110,
    render: (v: boolean) =>
      v ? (
        <Tag color="success" icon={<CheckCircleOutlined />}>
          Yes
        </Tag>
      ) : (
        <Tag color="default" icon={<CloseCircleOutlined />}>
          No
        </Tag>
      ),
    filters: [
      { text: 'Yes', value: true },
      { text: 'No', value: false },
    ],
    onFilter: (value: boolean | React.Key, record: LineageRecord) => record.contributed === value,
  },
];

function ByResponseTab() {
  const [responseId, setResponseId] = useState('');
  const [activeId, setActiveId] = useState('');
  const [records, setRecords] = useState<LineageRecord[]>([]);
  const [loading, setLoading] = useState(false);

  const handleSearch = async () => {
    const id = responseId.trim();
    if (!id) return;
    setLoading(true);
    try {
      const data = await getLineageByResponse(id);
      setRecords(data);
      setActiveId(id);
      if (data.length === 0) {
        message.info('No lineage records found for this response ID');
      }
    } catch {
      message.error('Failed to fetch lineage records');
    } finally {
      setLoading(false);
    }
  };

  return (
    <>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Space data-tour="lineage-input">
          <Input
            placeholder="Enter response ID..."
            value={responseId}
            onChange={(e) => setResponseId(e.target.value)}
            onPressEnter={handleSearch}
            style={{ width: 380 }}
            allowClear
          />
          <Button
            type="primary"
            icon={<SearchOutlined />}
            onClick={handleSearch}
            loading={loading}
          >
            Look Up
          </Button>
        </Space>
      </Card>

      {loading && <Spin />}

      {!loading && activeId && records.length === 0 && (
        <Empty description={`No lineage found for response: ${activeId}`} />
      )}

      {!loading && records.length > 0 && (
        <div data-tour="lineage-results">
          <Table<LineageRecord>
            dataSource={records}
            rowKey={(r) => `${r.response_id}-${r.chunk_id}`}
            columns={lineageColumns}
            pagination={{ pageSize: 20 }}
            size="small"
            scroll={{ x: 'max-content' }}
          />
        </div>
      )}
    </>
  );
}

function ByDocumentTab() {
  const [docId, setDocId] = useState('');
  const [activeId, setActiveId] = useState('');
  const [records, setRecords] = useState<LineageRecord[]>([]);
  const [loading, setLoading] = useState(false);

  const handleSearch = async () => {
    const id = docId.trim();
    if (!id) return;
    setLoading(true);
    try {
      const data = await getLineageByDocument(id, 50);
      setRecords(data);
      setActiveId(id);
      if (data.length === 0) {
        message.info('No lineage records found for this document ID');
      }
    } catch {
      message.error('Failed to fetch lineage records');
    } finally {
      setLoading(false);
    }
  };

  return (
    <>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Space>
          <Input
            placeholder="Enter document ID..."
            value={docId}
            onChange={(e) => setDocId(e.target.value)}
            onPressEnter={handleSearch}
            style={{ width: 380 }}
            allowClear
          />
          <Button
            type="primary"
            icon={<SearchOutlined />}
            onClick={handleSearch}
            loading={loading}
          >
            Look Up
          </Button>
        </Space>
      </Card>

      {loading && <Spin />}

      {!loading && activeId && records.length === 0 && (
        <Empty description={`No lineage found for document: ${activeId}`} />
      )}

      {!loading && records.length > 0 && (
        <Table<LineageRecord>
          dataSource={records}
          rowKey={(r) => `${r.response_id}-${r.chunk_id}`}
          columns={lineageColumns}
          pagination={{ pageSize: 20 }}
          size="small"
          scroll={{ x: 'max-content' }}
        />
      )}
    </>
  );
}

export default function LineagePage() {
  const { t } = useI18n();
  const tour = useTour('lineage');

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16 }}>
        <ApartmentOutlined style={{ fontSize: 18 }} />
        <Typography.Title level={4} style={{ margin: 0 }}>
          Lineage
        </Typography.Title>
        <TourGuideButton tourId="lineage" />
      </div>

      <Tabs
        data-tour="lineage-tabs"
        defaultActiveKey="response"
        items={[
          {
            key: 'response',
            label: 'By Response',
            children: <ByResponseTab />,
          },
          {
            key: 'document',
            label: 'By Document',
            children: <ByDocumentTab />,
          },
        ]}
      />
      <Tour
        open={tour.isActive}
        steps={getLineageSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
