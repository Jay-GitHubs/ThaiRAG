import { useState, useEffect } from 'react';
import {
  Typography,
  Card,
  Table,
  Space,
  Tabs,
  Button,
  Select,
  DatePicker,
  Tag,
  Statistic,
  Row,
  Col,
  Empty,
  Spin,
  Tooltip,
  message,
  theme,
} from 'antd';
import {
  AuditOutlined,
  CloudDownloadOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  QuestionCircleOutlined,
  BarChartOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  exportAuditLog,
  getAuditAnalytics,
} from '../api/auditLog';
import type { AuditLogEntry, AuditAnalytics } from '../api/auditLog';

const { RangePicker } = DatePicker;

// ── Helpers ───────────────────────────────────────────────────────

function downloadBlob(content: string, filename: string, mime: string) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

// ── Simple bar chart ─────────────────────────────────────────────

function SimpleBarChart({
  data,
  labelKey,
  valueKey,
  color = '#1677ff',
  height = 200,
}: {
  data: Record<string, unknown>[];
  labelKey: string;
  valueKey: string;
  color?: string;
  height?: number;
}) {
  const max = Math.max(...data.map((d) => Number(d[valueKey]) || 0), 1);
  const { token } = theme.useToken();

  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 2, height, padding: '0 4px' }}>
      {data.map((d, i) => {
        const val = Number(d[valueKey]) || 0;
        const barH = (val / max) * (height - 28);
        const label = String(d[labelKey]);
        return (
          <Tooltip key={i} title={`${label}: ${val.toLocaleString()}`}>
            <div
              style={{
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'flex-end',
                height: '100%',
              }}
            >
              <div
                style={{
                  width: '100%',
                  height: Math.max(barH, 1),
                  backgroundColor: color,
                  borderRadius: '2px 2px 0 0',
                  minWidth: 4,
                }}
              />
              <div
                style={{
                  fontSize: 9,
                  color: token.colorTextSecondary,
                  marginTop: 2,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  maxWidth: '100%',
                  textAlign: 'center',
                }}
              >
                {data.length <= 14 ? label : i % Math.ceil(data.length / 10) === 0 ? label : ''}
              </div>
            </div>
          </Tooltip>
        );
      })}
    </div>
  );
}

// ── Log Browser Tab ───────────────────────────────────────────────

function LogBrowserTab() {
  const [dateRange, setDateRange] = useState<[Dayjs, Dayjs] | null>(null);
  const [action, setAction] = useState<string | undefined>();
  const [entries, setEntries] = useState<AuditLogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);

  const buildFilter = () => ({
    from: dateRange?.[0]?.toISOString(),
    to: dateRange?.[1]?.toISOString(),
    action,
  });

  const fetchLogs = async () => {
    setLoading(true);
    try {
      const data = await exportAuditLog({ ...buildFilter(), format: 'json' });
      setEntries(data as AuditLogEntry[]);
    } catch {
      message.error('Failed to fetch audit logs');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLogs();
  }, [dateRange, action]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleExportJson = async () => {
    setExporting(true);
    try {
      const data = await exportAuditLog({ ...buildFilter(), format: 'json' });
      const json = JSON.stringify(data, null, 2);
      const ts = dayjs().format('YYYYMMDD-HHmmss');
      downloadBlob(json, `audit-log-${ts}.json`, 'application/json');
      message.success('Exported audit log as JSON');
    } catch {
      message.error('Export failed');
    } finally {
      setExporting(false);
    }
  };

  const handleExportCsv = async () => {
    setExporting(true);
    try {
      const data = await exportAuditLog({ ...buildFilter(), format: 'csv' });
      const ts = dayjs().format('YYYYMMDD-HHmmss');
      downloadBlob(String(data), `audit-log-${ts}.csv`, 'text/csv');
      message.success('Exported audit log as CSV');
    } catch {
      message.error('Export failed');
    } finally {
      setExporting(false);
    }
  };

  const columns = [
    {
      title: 'Date',
      dataIndex: 'timestamp',
      width: 170,
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: 'User',
      dataIndex: 'user_email',
      width: 200,
      ellipsis: true,
      render: (v?: string) => v ?? '-',
    },
    {
      title: 'Action',
      dataIndex: 'action',
      width: 160,
      render: (v: string) => <Tag>{v}</Tag>,
    },
    {
      title: 'Detail',
      dataIndex: 'detail',
      ellipsis: true,
    },
    {
      title: 'Success',
      dataIndex: 'success',
      width: 90,
      render: (v: boolean) =>
        v ? (
          <Tag color="success" icon={<CheckCircleOutlined />}>
            Yes
          </Tag>
        ) : (
          <Tag color="error" icon={<CloseCircleOutlined />}>
            No
          </Tag>
        ),
      filters: [
        { text: 'Yes', value: true },
        { text: 'No', value: false },
      ],
      onFilter: (value: boolean | React.Key, record: AuditLogEntry) => record.success === value,
    },
  ];

  return (
    <>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Space wrap>
          <RangePicker
            value={dateRange}
            onChange={(v) => setDateRange(v as [Dayjs, Dayjs] | null)}
            presets={[
              { label: 'Today', value: [dayjs().startOf('day'), dayjs()] },
              { label: 'Last 7 Days', value: [dayjs().subtract(7, 'day'), dayjs()] },
              { label: 'Last 30 Days', value: [dayjs().subtract(30, 'day'), dayjs()] },
            ]}
            size="small"
          />
          <Select
            placeholder="Action type"
            value={action}
            onChange={(v) => setAction(v)}
            allowClear
            size="small"
            style={{ width: 180 }}
            options={[
              { label: 'Login', value: 'login' },
              { label: 'Logout', value: 'logout' },
              { label: 'Create', value: 'create' },
              { label: 'Update', value: 'update' },
              { label: 'Delete', value: 'delete' },
              { label: 'Export', value: 'export' },
              { label: 'Settings Change', value: 'settings_change' },
            ]}
          />
          <Button icon={<CloudDownloadOutlined />} onClick={handleExportJson} loading={exporting} size="small">
            Export JSON
          </Button>
          <Button icon={<CloudDownloadOutlined />} onClick={handleExportCsv} loading={exporting} size="small">
            Export CSV
          </Button>
        </Space>
      </Card>

      <Table<AuditLogEntry>
        dataSource={entries}
        rowKey="id"
        columns={columns}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `${t} events` }}
        size="small"
        scroll={{ x: 'max-content' }}
      />
    </>
  );
}

// ── Analytics Tab ─────────────────────────────────────────────────

function AnalyticsTab() {
  const [dateRange, setDateRange] = useState<[Dayjs, Dayjs] | null>(null);
  const [analytics, setAnalytics] = useState<AuditAnalytics | null>(null);
  const [loading, setLoading] = useState(true);

  const filter = dateRange
    ? { from: dateRange[0].toISOString(), to: dateRange[1].toISOString() }
    : undefined;

  useEffect(() => {
    setLoading(true);
    getAuditAnalytics(filter)
      .then(setAnalytics)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [dateRange]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Space>
          <Typography.Text strong>Date Range:</Typography.Text>
          <RangePicker
            value={dateRange}
            onChange={(v) => setDateRange(v as [Dayjs, Dayjs] | null)}
            presets={[
              { label: 'Last 7 Days', value: [dayjs().subtract(7, 'day'), dayjs()] },
              { label: 'Last 30 Days', value: [dayjs().subtract(30, 'day'), dayjs()] },
            ]}
            size="small"
          />
        </Space>
      </Card>

      {loading && <Spin />}

      {!loading && !analytics && <Empty description="No analytics data available" />}

      {!loading && analytics && (
        <>
          <Row gutter={[16, 16]} style={{ marginBottom: 16 }}>
            <Col xs={24} sm={12}>
              <Card>
                <Statistic title="Total Events" value={analytics.total_events} />
              </Card>
            </Col>
            <Col xs={24} sm={12}>
              <Card>
                <Statistic
                  title="Success Rate"
                  value={((analytics.success_rate ?? 1) * 100).toFixed(1)}
                  suffix="%"
                  valueStyle={{
                    color:
                      (analytics.success_rate ?? 1) >= 0.95
                        ? '#52c41a'
                        : (analytics.success_rate ?? 1) >= 0.8
                        ? '#faad14'
                        : '#cf1322',
                  }}
                />
              </Card>
            </Col>
          </Row>

          <Row gutter={[16, 16]}>
            <Col xs={24} lg={12}>
              <Card title="Events by Action Type">
                {(analytics.actions_by_type ?? []).length > 0 ? (
                  <SimpleBarChart
                    data={(analytics.actions_by_type ?? []).map(([action, count]) => ({ action, count })) as unknown as Record<string, unknown>[]}
                    labelKey="action"
                    valueKey="count"
                    color="#1677ff"
                    height={220}
                  />
                ) : (
                  <Empty description="No data" />
                )}
              </Card>
            </Col>
            <Col xs={24} lg={12}>
              <Card title="Events Per Day">
                {(analytics.events_per_day ?? []).length > 0 ? (
                  <SimpleBarChart
                    data={(analytics.events_per_day ?? []).map(([date, count]) => ({ date, count })) as unknown as Record<string, unknown>[]}
                    labelKey="date"
                    valueKey="count"
                    color="#52c41a"
                    height={220}
                  />
                ) : (
                  <Empty description="No data" />
                )}
              </Card>
            </Col>
          </Row>
        </>
      )}
    </>
  );
}

// ── Main Page ─────────────────────────────────────────────────────

export default function AuditLogPage() {
  return (
    <>
      <Space align="baseline" style={{ marginBottom: 16 }}>
        <AuditOutlined style={{ fontSize: 18 }} />
        <Typography.Title level={4} style={{ margin: 0 }}>
          Audit Log
        </Typography.Title>
        <Tooltip title="A tamper-evident record of all admin and system actions for compliance and security review.">
          <QuestionCircleOutlined style={{ fontSize: 16 }} />
        </Tooltip>
      </Space>

      <Tabs
        defaultActiveKey="logs"
        items={[
          {
            key: 'logs',
            label: (
              <span>
                <AuditOutlined /> Log Browser
              </span>
            ),
            children: <LogBrowserTab />,
          },
          {
            key: 'analytics',
            label: (
              <span>
                <BarChartOutlined /> Analytics
              </span>
            ),
            children: <AnalyticsTab />,
          },
        ]}
      />
    </>
  );
}
