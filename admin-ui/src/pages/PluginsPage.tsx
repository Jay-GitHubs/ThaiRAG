import { useState, useEffect, useCallback } from 'react';
import { Card, Table, Switch, Tag, Typography, message, Space, Button } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { listPlugins, enablePlugin, disablePlugin } from '../api/plugins';
import type { PluginInfo } from '../api/plugins';

const { Title } = Typography;

export default function PluginsPage() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [toggling, setToggling] = useState<string | null>(null);

  const fetchPlugins = useCallback(async () => {
    setLoading(true);
    try {
      const res = await listPlugins();
      setPlugins(res.plugins);
    } catch {
      message.error('Failed to load plugins');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchPlugins();
  }, [fetchPlugins]);

  const handleToggle = async (name: string, enable: boolean) => {
    setToggling(name);
    try {
      if (enable) {
        await enablePlugin(name);
      } else {
        await disablePlugin(name);
      }
      await fetchPlugins();
    } catch {
      message.error(`Failed to ${enable ? 'enable' : 'disable'} plugin`);
    } finally {
      setToggling(null);
    }
  };

  const columns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
    },
    {
      title: 'Description',
      dataIndex: 'description',
      key: 'description',
    },
    {
      title: 'Type',
      dataIndex: 'plugin_type',
      key: 'plugin_type',
      render: (type: string) => <Tag color="blue">{type}</Tag>,
    },
    {
      title: 'Enabled',
      dataIndex: 'enabled',
      key: 'enabled',
      render: (enabled: boolean, record: PluginInfo) => (
        <Switch
          checked={enabled}
          loading={toggling === record.name}
          onChange={(checked) => handleToggle(record.name, checked)}
        />
      ),
    },
  ];

  return (
    <>
      <Space style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <Title level={3} style={{ margin: 0 }}>
          Plugins ({plugins.length})
        </Title>
        <Button icon={<ReloadOutlined />} onClick={fetchPlugins} loading={loading}>
          Refresh
        </Button>
      </Space>
      <Card>
        <Table
          dataSource={plugins}
          columns={columns}
          rowKey="name"
          loading={loading}
          pagination={false}
        />
      </Card>
    </>
  );
}
