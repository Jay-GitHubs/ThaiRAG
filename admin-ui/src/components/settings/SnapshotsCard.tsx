import { useState, useEffect } from 'react';
import {
  Card, Table, Button, Space, Typography, Tag, Modal, Input, Collapse,
  Popconfirm, message, Empty,
} from 'antd';
import {
  SaveOutlined, UndoOutlined, DeleteOutlined,
  CameraOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { listSnapshots, createSnapshot, restoreSnapshot, deleteSnapshot } from '../../api/settings';
import type { SnapshotListItem } from '../../api/types';

export function SnapshotsCard() {
  const [snapshots, setSnapshots] = useState<SnapshotListItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [restoring, setRestoring] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      setSnapshots(await listSnapshots());
    } catch {
      message.error('Failed to load snapshots');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, []);

  const handleCreate = async () => {
    if (!name.trim()) {
      message.warning('Please enter a name');
      return;
    }
    setCreating(true);
    try {
      await createSnapshot({ name: name.trim(), description: description.trim() });
      message.success('Snapshot saved');
      setModalOpen(false);
      setName('');
      setDescription('');
      load();
    } catch {
      message.error('Failed to create snapshot');
    } finally {
      setCreating(false);
    }
  };

  const handleRestore = async (
    id: string,
    opts?: { force?: boolean; skipEmbedding?: boolean },
  ) => {
    setRestoring(id);
    try {
      const result = await restoreSnapshot(id, opts);
      if (result.status === 'warning' && result.warning) {
        // Show modal with two options: skip embedding (safe) or force (destructive)
        Modal.confirm({
          title: 'Embedding Model Mismatch',
          width: 520,
          content: (
            <Space direction="vertical" size="middle" style={{ width: '100%', marginTop: 8 }}>
              <Typography.Text>
                The snapshot uses a different embedding model than your current configuration.
              </Typography.Text>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {result.warning.split('\n')[0]}
              </Typography.Text>
            </Space>
          ),
          okText: 'Restore Without Embedding (Safe)',
          cancelText: 'Cancel',
          onOk: () => handleRestore(id, { skipEmbedding: true }),
          footer: (_, { OkBtn, CancelBtn }) => (
            <Space style={{ width: '100%', justifyContent: 'flex-end', display: 'flex' }}>
              <CancelBtn />
              <OkBtn />
              <Button
                danger
                size="small"
                onClick={() => {
                  Modal.destroyAll();
                  handleRestore(id, { force: true });
                }}
              >
                Restore Everything (Re-index Required)
              </Button>
            </Space>
          ),
        });
      } else {
        message.success('Configuration restored! Reload the page to see updated settings.');
      }
    } catch {
      message.error('Failed to restore snapshot');
    } finally {
      setRestoring(null);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteSnapshot(id);
      message.success('Snapshot deleted');
      load();
    } catch {
      message.error('Failed to delete snapshot');
    }
  };

  const columns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (v: string, r: SnapshotListItem) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{v}</Typography.Text>
          {r.description && (
            <Typography.Text type="secondary" style={{ fontSize: 11 }}>
              {r.description}
            </Typography.Text>
          )}
        </Space>
      ),
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 160,
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: 'Embedding',
      dataIndex: 'embedding_fingerprint',
      key: 'embedding_fingerprint',
      width: 200,
      render: (v: string) => <Tag>{v}</Tag>,
    },
    {
      title: 'Settings',
      dataIndex: 'settings_count',
      key: 'settings_count',
      width: 80,
      render: (v: number) => <Tag>{v} keys</Tag>,
    },
    {
      title: '',
      key: 'actions',
      width: 160,
      render: (_: unknown, r: SnapshotListItem) => (
        <Space size="small">
          <Button
            size="small"
            icon={<UndoOutlined />}
            onClick={() => handleRestore(r.id, undefined)}
            loading={restoring === r.id}
          >
            Restore
          </Button>
          <Popconfirm
            title="Delete this snapshot?"
            onConfirm={() => handleDelete(r.id)}
          >
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <Collapse
      style={{ marginBottom: 16 }}
      items={[{
        key: 'snapshots',
        label: (
          <Space>
            <CameraOutlined />
            <span>Config Snapshots</span>
            {snapshots.length > 0 && <Tag>{snapshots.length}</Tag>}
          </Space>
        ),
        extra: (
          <Button
            size="small"
            icon={<SaveOutlined />}
            onClick={(e) => { e.stopPropagation(); setModalOpen(true); }}
          >
            Save Current Config
          </Button>
        ),
        children: (
          <>
            {snapshots.length === 0 && !loading ? (
              <Empty description="No snapshots yet. Save your current configuration to create a restore point." />
            ) : (
              <Table<SnapshotListItem>
                rowKey="id"
                columns={columns}
                dataSource={snapshots}
                loading={loading}
                pagination={false}
                size="small"
              />
            )}
            <Modal
              title="Save Configuration Snapshot"
              open={modalOpen}
              onOk={handleCreate}
              onCancel={() => { setModalOpen(false); setName(''); setDescription(''); }}
              okText="Save Snapshot"
              confirmLoading={creating}
            >
              <Space direction="vertical" style={{ width: '100%' }} size="middle">
                <div>
                  <Typography.Text strong style={{ display: 'block', marginBottom: 4 }}>
                    Name *
                  </Typography.Text>
                  <Input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="e.g. Before switching to Claude"
                    maxLength={100}
                  />
                </div>
                <div>
                  <Typography.Text strong style={{ display: 'block', marginBottom: 4 }}>
                    Description
                  </Typography.Text>
                  <Input.TextArea
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    placeholder="Optional notes about this configuration..."
                    rows={3}
                    maxLength={500}
                  />
                </div>
              </Space>
            </Modal>
          </>
        ),
      }]}
    />
  );
}
