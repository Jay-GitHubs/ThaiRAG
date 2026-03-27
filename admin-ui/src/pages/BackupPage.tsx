import { useState } from 'react';
import {
  Card,
  Typography,
  Button,
  Checkbox,
  Space,
  Upload,
  Modal,
  Descriptions,
  Alert,
  message,
  Spin,
  Divider,
  Tag,
  List,
} from 'antd';
import {
  CloudDownloadOutlined,
  CloudUploadOutlined,
  EyeOutlined,
  CheckCircleOutlined,
  WarningOutlined,
} from '@ant-design/icons';
import type { UploadFile } from 'antd';
import client from '../api/client';

const { Title, Text } = Typography;
const { Dragger } = Upload;

interface BackupManifest {
  version: string;
  created_at: string;
  includes: {
    settings: boolean;
    users: boolean;
    documents: boolean;
    org_structure: boolean;
  };
  stats: {
    users_count: number;
    orgs_count: number;
    depts_count: number;
    workspaces_count: number;
    documents_count: number;
    settings_count: number;
  };
}

interface BackupPreview {
  manifest: BackupManifest;
  files: string[];
}

interface RestoreResult {
  restored_settings: number;
  restored_users: number;
  restored_orgs: number;
  restored_depts: number;
  restored_workspaces: number;
  skipped: number;
  errors: string[];
}

export default function BackupPage() {
  // Create backup state
  const [includeSettings, setIncludeSettings] = useState(true);
  const [includeUsers, setIncludeUsers] = useState(true);
  const [includeDocuments, setIncludeDocuments] = useState(true);
  const [includeOrgStructure, setIncludeOrgStructure] = useState(true);
  const [creating, setCreating] = useState(false);

  // Restore state
  const [uploadFile, setUploadFile] = useState<UploadFile | null>(null);
  const [uploadBytes, setUploadBytes] = useState<Blob | null>(null);
  const [preview, setPreview] = useState<BackupPreview | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [skipExisting, setSkipExisting] = useState(true);
  const [restoreResult, setRestoreResult] = useState<RestoreResult | null>(null);
  const [showConfirm, setShowConfirm] = useState(false);

  const handleCreateBackup = async () => {
    setCreating(true);
    try {
      const res = await client.post(
        '/api/km/admin/backup',
        {
          include_settings: includeSettings,
          include_users: includeUsers,
          include_documents: includeDocuments,
          include_org_structure: includeOrgStructure,
        },
        { responseType: 'blob' },
      );

      // Extract filename from Content-Disposition header
      const disposition = res.headers['content-disposition'] || '';
      const match = disposition.match(/filename="?([^"]+)"?/);
      const filename = match ? match[1] : 'thairag-backup.zip';

      // Trigger download
      const url = window.URL.createObjectURL(new Blob([res.data]));
      const link = document.createElement('a');
      link.href = url;
      link.download = filename;
      document.body.appendChild(link);
      link.click();
      link.remove();
      window.URL.revokeObjectURL(url);

      message.success('Backup created and downloaded successfully');
    } catch (err: unknown) {
      const msg = (err as { response?: { data?: { error?: { message?: string } } } })
        ?.response?.data?.error?.message || 'Failed to create backup';
      message.error(msg);
    } finally {
      setCreating(false);
    }
  };

  const handlePreview = async (file: Blob) => {
    setPreviewing(true);
    try {
      const formData = new FormData();
      formData.append('file', file);

      const res = await client.post('/api/km/admin/backup/preview', formData, {
        headers: { 'Content-Type': 'multipart/form-data' },
      });
      setPreview(res.data);
    } catch (err: unknown) {
      const msg = (err as { response?: { data?: { error?: { message?: string } } } })
        ?.response?.data?.error?.message || 'Failed to preview backup';
      message.error(msg);
    } finally {
      setPreviewing(false);
    }
  };

  const handleRestore = async () => {
    if (!uploadBytes) return;
    setShowConfirm(false);
    setRestoring(true);
    try {
      const formData = new FormData();
      formData.append('file', uploadBytes);

      const res = await client.post(
        `/api/km/admin/restore?skip_existing=${skipExisting}`,
        formData,
        { headers: { 'Content-Type': 'multipart/form-data' } },
      );
      setRestoreResult(res.data);
      message.success('Backup restored successfully');
    } catch (err: unknown) {
      const msg = (err as { response?: { data?: { error?: { message?: string } } } })
        ?.response?.data?.error?.message || 'Failed to restore backup';
      message.error(msg);
    } finally {
      setRestoring(false);
    }
  };

  const resetRestore = () => {
    setUploadFile(null);
    setUploadBytes(null);
    setPreview(null);
    setRestoreResult(null);
  };

  return (
    <>
      <Title level={4}>Backup & Restore</Title>

      {/* ── Create Backup ─────────────────────────────────────────── */}
      <Card
        title={
          <Space>
            <CloudDownloadOutlined />
            <span>Create Backup</span>
          </Space>
        }
        style={{ marginBottom: 16 }}
      >
        <Text type="secondary" style={{ display: 'block', marginBottom: 16 }}>
          Create a full system backup as a downloadable ZIP file. Select which
          data to include:
        </Text>

        <Space direction="vertical" style={{ marginBottom: 16 }}>
          <Checkbox checked={includeSettings} onChange={(e) => setIncludeSettings(e.target.checked)}>
            Settings (global config, provider settings)
          </Checkbox>
          <Checkbox checked={includeUsers} onChange={(e) => setIncludeUsers(e.target.checked)}>
            Users (accounts, roles)
          </Checkbox>
          <Checkbox checked={includeOrgStructure} onChange={(e) => setIncludeOrgStructure(e.target.checked)}>
            Organization Structure (orgs, depts, workspaces, permissions, identity providers)
          </Checkbox>
          <Checkbox checked={includeDocuments} onChange={(e) => setIncludeDocuments(e.target.checked)}>
            Document Metadata (titles, status &mdash; not file content)
          </Checkbox>
        </Space>

        <div>
          <Button
            type="primary"
            icon={<CloudDownloadOutlined />}
            loading={creating}
            onClick={handleCreateBackup}
          >
            Create & Download Backup
          </Button>
        </div>
      </Card>

      {/* ── Restore from Backup ───────────────────────────────────── */}
      <Card
        title={
          <Space>
            <CloudUploadOutlined />
            <span>Restore from Backup</span>
          </Space>
        }
      >
        <Text type="secondary" style={{ display: 'block', marginBottom: 16 }}>
          Upload a ThaiRAG backup ZIP file to preview and restore system state.
        </Text>

        {!preview && !restoreResult && (
          <Dragger
            accept=".zip"
            maxCount={1}
            fileList={uploadFile ? [uploadFile] : []}
            beforeUpload={(file) => {
              const blob = new Blob([file], { type: file.type });
              setUploadFile({
                uid: file.uid,
                name: file.name,
                size: file.size,
                type: file.type,
              } as UploadFile);
              setUploadBytes(blob);
              handlePreview(blob);
              return false; // prevent auto-upload
            }}
            onRemove={() => {
              resetRestore();
            }}
          >
            <p className="ant-upload-drag-icon">
              <CloudUploadOutlined />
            </p>
            <p className="ant-upload-text">
              Click or drag a .zip backup file to this area
            </p>
          </Dragger>
        )}

        {previewing && (
          <div style={{ textAlign: 'center', padding: 24 }}>
            <Spin tip="Analyzing backup..." />
          </div>
        )}

        {/* ── Preview ──────────────────────────────────────────────── */}
        {preview && !restoreResult && (
          <>
            <Alert
              message="Backup Preview"
              description={`Backup v${preview.manifest.version} created on ${new Date(preview.manifest.created_at).toLocaleString()}`}
              type="info"
              showIcon
              icon={<EyeOutlined />}
              style={{ marginBottom: 16, marginTop: 16 }}
            />

            <Descriptions bordered size="small" column={2} style={{ marginBottom: 16 }}>
              <Descriptions.Item label="Settings">
                {preview.manifest.includes.settings ? (
                  <Tag color="green">{preview.manifest.stats.settings_count} entries</Tag>
                ) : (
                  <Tag>Not included</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label="Users">
                {preview.manifest.includes.users ? (
                  <Tag color="green">{preview.manifest.stats.users_count} users</Tag>
                ) : (
                  <Tag>Not included</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label="Organizations">
                {preview.manifest.includes.org_structure ? (
                  <Tag color="green">{preview.manifest.stats.orgs_count} orgs</Tag>
                ) : (
                  <Tag>Not included</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label="Departments">
                {preview.manifest.includes.org_structure ? (
                  <Tag color="green">{preview.manifest.stats.depts_count} depts</Tag>
                ) : (
                  <Tag>Not included</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label="Workspaces">
                {preview.manifest.includes.org_structure ? (
                  <Tag color="green">{preview.manifest.stats.workspaces_count} workspaces</Tag>
                ) : (
                  <Tag>Not included</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label="Documents">
                {preview.manifest.includes.documents ? (
                  <Tag color="green">{preview.manifest.stats.documents_count} docs</Tag>
                ) : (
                  <Tag>Not included</Tag>
                )}
              </Descriptions.Item>
            </Descriptions>

            <Descriptions bordered size="small" column={1} style={{ marginBottom: 16 }}>
              <Descriptions.Item label="Files in archive">
                {preview.files.join(', ')}
              </Descriptions.Item>
            </Descriptions>

            <Divider />

            <Space direction="vertical" style={{ marginBottom: 16 }}>
              <Checkbox checked={skipExisting} onChange={(e) => setSkipExisting(e.target.checked)}>
                Skip existing records (don't overwrite)
              </Checkbox>
            </Space>

            <Space>
              <Button
                type="primary"
                icon={<CloudUploadOutlined />}
                loading={restoring}
                onClick={() => setShowConfirm(true)}
              >
                Restore Backup
              </Button>
              <Button onClick={resetRestore}>Cancel</Button>
            </Space>
          </>
        )}

        {/* ── Restore Result ─────────────────────────────────────── */}
        {restoreResult && (
          <>
            <Alert
              message="Restore Complete"
              type={restoreResult.errors.length > 0 ? 'warning' : 'success'}
              showIcon
              icon={restoreResult.errors.length > 0 ? <WarningOutlined /> : <CheckCircleOutlined />}
              style={{ marginBottom: 16, marginTop: 16 }}
            />

            <Descriptions bordered size="small" column={2} style={{ marginBottom: 16 }}>
              <Descriptions.Item label="Settings Restored">
                <Tag color="green">{restoreResult.restored_settings}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Users Restored">
                <Tag color="green">{restoreResult.restored_users}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Orgs Restored">
                <Tag color="green">{restoreResult.restored_orgs}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Depts Restored">
                <Tag color="green">{restoreResult.restored_depts}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Workspaces Restored">
                <Tag color="green">{restoreResult.restored_workspaces}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Skipped">
                <Tag color="orange">{restoreResult.skipped}</Tag>
              </Descriptions.Item>
            </Descriptions>

            {restoreResult.errors.length > 0 && (
              <List
                header={<Text strong>Errors ({restoreResult.errors.length})</Text>}
                bordered
                size="small"
                dataSource={restoreResult.errors}
                renderItem={(error) => (
                  <List.Item>
                    <Text type="danger">{error}</Text>
                  </List.Item>
                )}
                style={{ marginBottom: 16 }}
              />
            )}

            <Button onClick={resetRestore}>Upload Another Backup</Button>
          </>
        )}
      </Card>

      {/* ── Confirm Dialog ────────────────────────────────────────── */}
      <Modal
        title="Confirm Restore"
        open={showConfirm}
        onOk={handleRestore}
        onCancel={() => setShowConfirm(false)}
        okText="Yes, Restore"
        okButtonProps={{ danger: true }}
      >
        <Alert
          message="This will restore data from the backup into your system."
          description={
            skipExisting
              ? 'Existing records will be preserved (skip mode).'
              : 'Existing records may be overwritten. This cannot be undone.'
          }
          type={skipExisting ? 'info' : 'warning'}
          showIcon
          style={{ marginBottom: 16 }}
        />
        <p>Are you sure you want to proceed?</p>
      </Modal>
    </>
  );
}
