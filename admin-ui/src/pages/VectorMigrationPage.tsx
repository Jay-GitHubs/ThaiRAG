import { useState, useEffect, useCallback } from 'react';
import {
  Card,
  Typography,
  Button,
  Select,
  Input,
  Space,
  Alert,
  Progress,
  Descriptions,
  Tag,
  Divider,
  Modal,
  message,
  Spin,
} from 'antd';
import {
  SwapOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  PlayCircleOutlined,
  SyncOutlined,
} from '@ant-design/icons';
import client from '../api/client';

const { Title, Text } = Typography;

interface VectorStoreInfo {
  backend: string;
  collection_name: string;
  vector_count: number;
}

interface MigrationResult {
  total: number;
  migrated: number;
  failed: number;
  skipped: number;
  duration_ms: number;
}

interface ValidationResult {
  source_count: number;
  target_count: number;
  samples_checked: number;
  samples_matched: number;
  is_valid: boolean;
  message: string;
}

interface MigrationStatus {
  state: string;
  total: number;
  migrated: number;
  failed: number;
  target: {
    kind: string;
    url: string;
    collection: string;
  } | null;
  result: MigrationResult | null;
  validation: ValidationResult | null;
  error: string | null;
}

const VECTOR_STORE_KINDS = [
  { value: 'qdrant', label: 'Qdrant' },
  { value: 'pgvector', label: 'PgVector' },
  { value: 'chromadb', label: 'ChromaDB' },
  { value: 'pinecone', label: 'Pinecone' },
  { value: 'weaviate', label: 'Weaviate' },
  { value: 'milvus', label: 'Milvus' },
  { value: 'in_memory', label: 'In-Memory' },
];

export default function VectorMigrationPage() {
  // Current provider info
  const [currentInfo, setCurrentInfo] = useState<VectorStoreInfo | null>(null);
  const [loadingInfo, setLoadingInfo] = useState(false);

  // Target config
  const [targetKind, setTargetKind] = useState('qdrant');
  const [targetUrl, setTargetUrl] = useState('');
  const [targetCollection, setTargetCollection] = useState('');
  const [targetApiKey, setTargetApiKey] = useState('');
  const [batchSize, setBatchSize] = useState(100);

  // Migration status
  const [status, setStatus] = useState<MigrationStatus | null>(null);
  const [polling, setPolling] = useState(false);

  // Loading states
  const [starting, setStarting] = useState(false);
  const [validating, setValidating] = useState(false);
  const [switching, setSwitching] = useState(false);

  const loadCurrentInfo = useCallback(async () => {
    setLoadingInfo(true);
    try {
      const res = await client.get('/api/km/settings/vectordb/info');
      setCurrentInfo(res.data);
    } catch {
      // ignore
    } finally {
      setLoadingInfo(false);
    }
  }, []);

  const loadStatus = useCallback(async () => {
    try {
      const res = await client.get('/api/km/admin/vector-migration/status');
      setStatus(res.data);
      return res.data;
    } catch {
      return null;
    }
  }, []);

  useEffect(() => {
    loadCurrentInfo();
    loadStatus();
  }, [loadCurrentInfo, loadStatus]);

  // Poll for status updates while migration is running
  useEffect(() => {
    if (!polling) return;
    const interval = setInterval(async () => {
      const s = await loadStatus();
      if (s && s.state !== 'running') {
        setPolling(false);
      }
    }, 2000);
    return () => clearInterval(interval);
  }, [polling, loadStatus]);

  const handleStartMigration = async () => {
    if (!targetKind) {
      message.error('Please select a target provider');
      return;
    }
    if (targetKind !== 'in_memory' && !targetUrl) {
      message.error('Please enter the target URL');
      return;
    }

    setStarting(true);
    try {
      await client.post('/api/km/admin/vector-migration/start', {
        target: {
          kind: targetKind,
          url: targetUrl,
          collection: targetCollection,
          api_key: targetApiKey,
        },
        batch_size: batchSize,
      });
      message.success('Migration started');
      setPolling(true);
      await loadStatus();
    } catch (err: unknown) {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Failed to start migration';
      message.error(msg);
    } finally {
      setStarting(false);
    }
  };

  const handleValidate = async () => {
    setValidating(true);
    try {
      const res = await client.post('/api/km/admin/vector-migration/validate');
      await loadStatus();
      if (res.data.is_valid) {
        message.success('Validation passed');
      } else {
        message.warning('Validation failed: ' + res.data.message);
      }
    } catch (err: unknown) {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Validation failed';
      message.error(msg);
    } finally {
      setValidating(false);
    }
  };

  const handleSwitch = () => {
    Modal.confirm({
      title: 'Switch Vector Store Provider',
      icon: <ExclamationCircleOutlined />,
      content:
        'This will switch the active vector store to the migration target. ' +
        'The previous vector store will no longer be used. Are you sure?',
      okText: 'Switch',
      okType: 'danger',
      onOk: async () => {
        setSwitching(true);
        try {
          const res = await client.post('/api/km/admin/vector-migration/switch');
          message.success(res.data.message);
          await loadCurrentInfo();
          await loadStatus();
        } catch (err: unknown) {
          const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Switch failed';
          message.error(msg);
        } finally {
          setSwitching(false);
        }
      },
    });
  };

  const isRunning = status?.state === 'running';
  const isCompleted = status?.state === 'completed';
  const isValidated = status?.state === 'validated';
  const isFailed = status?.state === 'failed';

  const progressPercent =
    status && status.total > 0
      ? Math.round((status.migrated / status.total) * 100)
      : 0;

  const needsUrl = targetKind !== 'in_memory';
  const needsCollection = !['in_memory', 'pinecone', 'pgvector'].includes(targetKind);

  return (
    <div>
      <Title level={2}>
        <SwapOutlined /> Vector Database Migration
      </Title>
      <Text type="secondary">
        Migrate vectors between vector store providers without re-embedding documents.
      </Text>

      <div style={{ marginTop: 24, display: 'flex', gap: 24, flexWrap: 'wrap' }}>
        {/* Current Provider Info */}
        <Card
          title="Current Vector Store"
          style={{ flex: 1, minWidth: 350 }}
          loading={loadingInfo}
          extra={
            <Button size="small" onClick={loadCurrentInfo} icon={<SyncOutlined />}>
              Refresh
            </Button>
          }
        >
          {currentInfo ? (
            <Descriptions column={1} size="small">
              <Descriptions.Item label="Backend">
                <Tag color="blue">{currentInfo.backend}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Collection">
                {currentInfo.collection_name}
              </Descriptions.Item>
              <Descriptions.Item label="Vector Count">
                <Text strong>{currentInfo.vector_count.toLocaleString()}</Text>
              </Descriptions.Item>
            </Descriptions>
          ) : (
            <Text type="secondary">Unable to load vector store info</Text>
          )}
        </Card>

        {/* Target Provider Config */}
        <Card title="Target Vector Store" style={{ flex: 1, minWidth: 350 }}>
          <Space direction="vertical" style={{ width: '100%' }} size="middle">
            <div>
              <Text strong>Provider</Text>
              <Select
                style={{ width: '100%', marginTop: 4 }}
                value={targetKind}
                onChange={setTargetKind}
                options={VECTOR_STORE_KINDS}
                disabled={isRunning}
              />
            </div>
            {needsUrl && (
              <div>
                <Text strong>URL</Text>
                <Input
                  style={{ marginTop: 4 }}
                  placeholder="http://localhost:6334"
                  value={targetUrl}
                  onChange={(e) => setTargetUrl(e.target.value)}
                  disabled={isRunning}
                />
              </div>
            )}
            {needsCollection && (
              <div>
                <Text strong>Collection Name</Text>
                <Input
                  style={{ marginTop: 4 }}
                  placeholder="thairag_vectors"
                  value={targetCollection}
                  onChange={(e) => setTargetCollection(e.target.value)}
                  disabled={isRunning}
                />
              </div>
            )}
            {['pinecone', 'weaviate'].includes(targetKind) && (
              <div>
                <Text strong>API Key</Text>
                <Input.Password
                  style={{ marginTop: 4 }}
                  placeholder="API key"
                  value={targetApiKey}
                  onChange={(e) => setTargetApiKey(e.target.value)}
                  disabled={isRunning}
                />
              </div>
            )}
            <div>
              <Text strong>Batch Size</Text>
              <Input
                type="number"
                style={{ marginTop: 4, width: 120 }}
                value={batchSize}
                onChange={(e) => setBatchSize(Number(e.target.value) || 100)}
                min={10}
                max={1000}
                disabled={isRunning}
              />
            </div>
          </Space>
        </Card>
      </div>

      <Divider />

      {/* Migration Controls */}
      <Card title="Migration Controls">
        <Space size="middle" wrap>
          <Button
            type="primary"
            icon={<PlayCircleOutlined />}
            onClick={handleStartMigration}
            loading={starting}
            disabled={isRunning}
            size="large"
          >
            Start Migration
          </Button>

          <Button
            icon={<CheckCircleOutlined />}
            onClick={handleValidate}
            loading={validating}
            disabled={!isCompleted && !isValidated}
          >
            Validate Migration
          </Button>

          <Button
            type="primary"
            danger
            icon={<SwapOutlined />}
            onClick={handleSwitch}
            loading={switching}
            disabled={!isCompleted && !isValidated}
          >
            Switch Provider
          </Button>
        </Space>

        {/* Progress */}
        {isRunning && (
          <div style={{ marginTop: 24 }}>
            <Spin spinning>
              <Text>Migration in progress...</Text>
            </Spin>
            <Progress
              percent={progressPercent}
              status="active"
              style={{ marginTop: 8 }}
              format={() =>
                `${status?.migrated?.toLocaleString() || 0} / ${status?.total?.toLocaleString() || 0}`
              }
            />
          </div>
        )}

        {/* Result */}
        {status?.result && (
          <div style={{ marginTop: 24 }}>
            <Alert
              type={status.result.failed === 0 ? 'success' : 'warning'}
              showIcon
              message="Migration Complete"
              description={
                <Descriptions column={2} size="small" style={{ marginTop: 8 }}>
                  <Descriptions.Item label="Total">{status.result.total.toLocaleString()}</Descriptions.Item>
                  <Descriptions.Item label="Migrated">{status.result.migrated.toLocaleString()}</Descriptions.Item>
                  <Descriptions.Item label="Failed">{status.result.failed.toLocaleString()}</Descriptions.Item>
                  <Descriptions.Item label="Skipped">{status.result.skipped.toLocaleString()}</Descriptions.Item>
                  <Descriptions.Item label="Duration">
                    {(status.result.duration_ms / 1000).toFixed(1)}s
                  </Descriptions.Item>
                </Descriptions>
              }
            />
          </div>
        )}

        {/* Validation Result */}
        {status?.validation && (
          <div style={{ marginTop: 16 }}>
            <Alert
              type={status.validation.is_valid ? 'success' : 'error'}
              showIcon
              message={status.validation.is_valid ? 'Validation Passed' : 'Validation Failed'}
              description={
                <div>
                  <p>{status.validation.message}</p>
                  <Text type="secondary">
                    Source: {status.validation.source_count.toLocaleString()} vectors |{' '}
                    Target: {status.validation.target_count.toLocaleString()} vectors
                  </Text>
                </div>
              }
            />
          </div>
        )}

        {/* Error */}
        {isFailed && status?.error && (
          <div style={{ marginTop: 16 }}>
            <Alert
              type="error"
              showIcon
              message="Migration Failed"
              description={status.error}
            />
          </div>
        )}
      </Card>
    </div>
  );
}
