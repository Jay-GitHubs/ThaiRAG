import { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Popconfirm,
  Space,
  Spin,
  Statistic,
  Tag,
  Typography,
  message,
} from 'antd';
import {
  DatabaseOutlined,
  DeleteOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import type { VectorDbInfo } from '../../api/types';
import { clearVectorDb, getVectorDbInfo } from '../../api/settings';

const backendColors: Record<string, string> = {
  InMemory: 'orange',
  Qdrant: 'blue',
  Pgvector: 'purple',
  ChromaDb: 'green',
  Pinecone: 'cyan',
  Weaviate: 'magenta',
  Milvus: 'volcano',
};

export function VectorDbTab() {
  const [info, setInfo] = useState<VectorDbInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [clearing, setClearing] = useState(false);

  const fetchInfo = async () => {
    setLoading(true);
    try {
      const data = await getVectorDbInfo();
      setInfo(data);
    } catch {
      message.error('Failed to load vector database info');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchInfo();
  }, []);

  const handleClear = async () => {
    setClearing(true);
    try {
      const res = await clearVectorDb();
      message.success(res.message);
      await fetchInfo();
    } catch {
      message.error('Failed to clear vector database');
    } finally {
      setClearing(false);
    }
  };

  if (loading && !info) {
    return <Spin tip="Loading vector database info..." />;
  }

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Card
        title={
          <Space>
            <DatabaseOutlined />
            <span>Vector Database</span>
          </Space>
        }
        extra={
          <Button icon={<ReloadOutlined />} onClick={fetchInfo} loading={loading}>
            Refresh
          </Button>
        }
      >
        {info && (
          <>
            <Descriptions column={2} bordered size="small">
              <Descriptions.Item label="Backend">
                <Tag color={backendColors[info.backend] || 'default'}>
                  {info.backend}
                </Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Isolation">
                <Tag>{info.isolation}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Collection">
                <Typography.Text code>{info.collection || '(default)'}</Typography.Text>
              </Descriptions.Item>
              <Descriptions.Item label="URL">
                <Typography.Text code copyable={!!info.url}>
                  {info.url || '(local)'}
                </Typography.Text>
              </Descriptions.Item>
            </Descriptions>

            <div style={{ marginTop: 16 }}>
              <Statistic
                title="Total Vectors"
                value={info.vector_count}
                suffix="vectors indexed"
              />
            </div>
          </>
        )}
      </Card>

      <Card title="Danger Zone" type="inner">
        <Alert
          message="Clearing the vector database will delete all indexed vectors. All documents will need to be re-processed to rebuild the search index."
          type="warning"
          showIcon
          style={{ marginBottom: 16 }}
        />
        <Popconfirm
          title="Clear all vectors?"
          description="This action cannot be undone. All indexed vectors will be permanently deleted."
          onConfirm={handleClear}
          okText="Yes, clear all"
          okButtonProps={{ danger: true }}
          cancelText="Cancel"
        >
          <Button
            danger
            icon={<DeleteOutlined />}
            loading={clearing}
          >
            Clear All Vectors
          </Button>
        </Popconfirm>
      </Card>
    </Space>
  );
}
