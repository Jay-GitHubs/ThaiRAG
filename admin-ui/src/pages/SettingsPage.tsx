import { Tabs, Typography } from 'antd';
import { DocumentProcessingTab } from '../components/settings/DocumentProcessingTab';
import { IdpTab } from '../components/settings/IdpTab';
import { LocalAuthTab } from '../components/settings/LocalAuthTab';
import { PresetsCard } from '../components/settings/PresetsCard';
import { PromptsTab } from '../components/settings/PromptsTab';
import { ProvidersTab } from '../components/settings/ProvidersTab';
import { SnapshotsCard } from '../components/settings/SnapshotsCard';
import { VaultTab } from '../components/settings/VaultTab';
import { VectorDbTab } from '../components/settings/VectorDbTab';

export function SettingsPage() {
  return (
    <>
      <Typography.Title level={4}>Settings</Typography.Title>
      <SnapshotsCard />
      <Tabs
        defaultActiveKey="presets"
        items={[
          { key: 'presets', label: 'Quick Setup', children: <PresetsCard /> },
          { key: 'vault', label: 'API Keys & Profiles', children: <VaultTab /> },
          { key: 'providers', label: 'Chat & Response Pipeline', children: <ProvidersTab /> },
          { key: 'documents', label: 'Document Processing', children: <DocumentProcessingTab /> },
          { key: 'vectordb', label: 'Vector Database', children: <VectorDbTab /> },
          { key: 'prompts', label: 'Agent Prompts', children: <PromptsTab /> },
          { key: 'idp', label: 'Identity Providers', children: <IdpTab /> },
          { key: 'local', label: 'Local Auth', children: <LocalAuthTab /> },
        ]}
      />
    </>
  );
}
