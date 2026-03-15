import { Tabs, Typography } from 'antd';
import { DocumentProcessingTab } from '../components/settings/DocumentProcessingTab';
import { IdpTab } from '../components/settings/IdpTab';
import { LocalAuthTab } from '../components/settings/LocalAuthTab';
import { PresetsCard } from '../components/settings/PresetsCard';
import { PromptsTab } from '../components/settings/PromptsTab';
import { ProvidersTab } from '../components/settings/ProvidersTab';

export function SettingsPage() {
  return (
    <>
      <Typography.Title level={4}>Settings</Typography.Title>
      <Tabs
        defaultActiveKey="presets"
        items={[
          { key: 'presets', label: 'Quick Setup', children: <PresetsCard /> },
          { key: 'providers', label: 'Chat & Response Pipeline', children: <ProvidersTab /> },
          { key: 'documents', label: 'Document Processing', children: <DocumentProcessingTab /> },
          { key: 'prompts', label: 'Agent Prompts', children: <PromptsTab /> },
          { key: 'idp', label: 'Identity Providers', children: <IdpTab /> },
          { key: 'local', label: 'Local Auth', children: <LocalAuthTab /> },
        ]}
      />
    </>
  );
}
