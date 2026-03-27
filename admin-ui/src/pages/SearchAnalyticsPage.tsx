import { useState, useEffect } from 'react';
import {
  Typography,
  Card,
  Row,
  Col,
  Statistic,
  Table,
  Space,
  DatePicker,
  Spin,
  Empty,
  Tooltip,
} from 'antd';
import {
  SearchOutlined,
  ThunderboltOutlined,
  FieldTimeOutlined,
  FileSearchOutlined,
  QuestionCircleOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import {
  getSearchAnalyticsSummary,
  getPopularQueries,
  getSearchAnalyticsEvents,
} from '../api/searchAnalytics';
import type {
  SearchAnalyticsSummary,
  PopularQuery,
  SearchAnalyticsEvent,
} from '../api/searchAnalytics';

const { RangePicker } = DatePicker;

export default function SearchAnalyticsPage() {
  const [dateRange, setDateRange] = useState<[Dayjs, Dayjs] | null>(null);
  const [summary, setSummary] = useState<SearchAnalyticsSummary | null>(null);
  const [popularQueries, setPopularQueries] = useState<PopularQuery[]>([]);
  const [zeroResultEvents, setZeroResultEvents] = useState<SearchAnalyticsEvent[]>([]);
  const [loading, setLoading] = useState(true);

  const filter = dateRange
    ? { from: dateRange[0].toISOString(), to: dateRange[1].toISOString() }
    : undefined;

  useEffect(() => {
    setLoading(true);
    Promise.all([
      getSearchAnalyticsSummary(filter),
      getPopularQueries(20, filter),
      getSearchAnalyticsEvents({ ...filter, zero_results_only: true, limit: 20 }),
    ])
      .then(([s, p, e]) => {
        setSummary(s);
        setPopularQueries(p);
        setZeroResultEvents(e);
      })
      .catch(() => {
        // keep previous data on error
      })
      .finally(() => setLoading(false));
  }, [dateRange]); // eslint-disable-line react-hooks/exhaustive-deps

  const popularColumns = [
    {
      title: '#',
      width: 50,
      render: (_: unknown, __: unknown, idx: number) => idx + 1,
    },
    {
      title: 'Query',
      dataIndex: 'query',
      ellipsis: true,
    },
    {
      title: 'Count',
      dataIndex: 'count',
      width: 90,
      sorter: (a: PopularQuery, b: PopularQuery) => a.count - b.count,
      defaultSortOrder: 'descend' as const,
    },
    {
      title: 'Avg Results',
      dataIndex: 'avg_results',
      width: 110,
      render: (v: number) => v.toFixed(1),
      sorter: (a: PopularQuery, b: PopularQuery) => a.avg_results - b.avg_results,
    },
    {
      title: 'Avg Latency',
      dataIndex: 'avg_latency_ms',
      width: 120,
      render: (v: number) => `${Math.round(v)}ms`,
      sorter: (a: PopularQuery, b: PopularQuery) => a.avg_latency_ms - b.avg_latency_ms,
    },
  ];

  const zeroResultColumns = [
    {
      title: 'Query',
      dataIndex: 'query',
      ellipsis: true,
    },
    {
      title: 'Latency',
      dataIndex: 'latency_ms',
      width: 100,
      render: (v: number) => `${Math.round(v)}ms`,
    },
    {
      title: 'Timestamp',
      dataIndex: 'timestamp',
      width: 170,
      render: (v: string) => dayjs(v).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: 'Workspace',
      dataIndex: 'workspace_id',
      width: 160,
      ellipsis: true,
      render: (v?: string) => v ?? '-',
    },
  ];

  return (
    <>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 16,
          flexWrap: 'wrap',
          gap: 8,
        }}
      >
        <Space align="baseline">
          <Typography.Title level={4} style={{ margin: 0 }}>
            Search Analytics
          </Typography.Title>
          <Tooltip title="Insights into search query patterns, zero-result queries, and latency trends.">
            <QuestionCircleOutlined style={{ fontSize: 16 }} />
          </Tooltip>
        </Space>
        <RangePicker
          value={dateRange}
          onChange={(v) => setDateRange(v as [Dayjs, Dayjs] | null)}
          presets={[
            { label: 'Today', value: [dayjs().startOf('day'), dayjs()] },
            { label: 'Last 7 Days', value: [dayjs().subtract(7, 'day'), dayjs()] },
            { label: 'Last 30 Days', value: [dayjs().subtract(30, 'day'), dayjs()] },
          ]}
        />
      </div>

      {loading && !summary ? (
        <div style={{ textAlign: 'center', paddingTop: 80 }}>
          <Spin size="large" />
          <div style={{ marginTop: 16 }}>
            <Typography.Text type="secondary">Loading search analytics...</Typography.Text>
          </div>
        </div>
      ) : !summary ? (
        <Empty description="No search analytics data available" />
      ) : (
        <>
          {/* Summary Stats */}
          <Row gutter={[16, 16]}>
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title="Total Searches"
                  value={summary.total_searches}
                  prefix={<ThunderboltOutlined />}
                />
              </Card>
            </Col>
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title={
                    <Tooltip title="Percentage of searches that returned zero results">
                      <span>
                        Zero-Result Rate <QuestionCircleOutlined />
                      </span>
                    </Tooltip>
                  }
                  value={(summary.zero_result_rate * 100).toFixed(1)}
                  suffix="%"
                  prefix={<SearchOutlined />}
                  valueStyle={{
                    color:
                      summary.zero_result_rate > 0.2
                        ? '#cf1322'
                        : summary.zero_result_rate > 0.1
                        ? '#faad14'
                        : '#52c41a',
                  }}
                />
              </Card>
            </Col>
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title="Avg Latency"
                  value={Math.round(summary.avg_latency_ms)}
                  suffix="ms"
                  prefix={<FieldTimeOutlined />}
                />
              </Card>
            </Col>
            <Col xs={24} sm={12} lg={6}>
              <Card>
                <Statistic
                  title="Avg Results"
                  value={summary.avg_results.toFixed(1)}
                  prefix={<FileSearchOutlined />}
                />
              </Card>
            </Col>
          </Row>

          {/* Popular Queries */}
          <Card title="Popular Queries" style={{ marginTop: 16 }} loading={loading}>
            {popularQueries.length > 0 ? (
              <Table<PopularQuery>
                dataSource={popularQueries}
                rowKey="query"
                columns={popularColumns}
                pagination={{ pageSize: 10 }}
                size="small"
                scroll={{ x: 'max-content' }}
              />
            ) : (
              <Empty description="No query data" />
            )}
          </Card>

          {/* Zero-Result Queries */}
          <Card title="Zero-Result Queries" style={{ marginTop: 16 }} loading={loading}>
            {zeroResultEvents.length > 0 ? (
              <Table<SearchAnalyticsEvent>
                dataSource={zeroResultEvents}
                rowKey="id"
                columns={zeroResultColumns}
                pagination={{ pageSize: 10 }}
                size="small"
                scroll={{ x: 'max-content' }}
              />
            ) : (
              <Empty description="No zero-result queries found" />
            )}
          </Card>
        </>
      )}
    </>
  );
}
