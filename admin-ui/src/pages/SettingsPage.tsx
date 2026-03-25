import { useState } from 'react';
import { Card, Tabs, Typography } from 'antd';
import type { SettingsScopeParam } from '../api/types';
import { DocumentProcessingTab } from '../components/settings/DocumentProcessingTab';
import { IdpTab } from '../components/settings/IdpTab';
import { LocalAuthTab } from '../components/settings/LocalAuthTab';
import { PresetsCard } from '../components/settings/PresetsCard';
import { PromptsTab } from '../components/settings/PromptsTab';
import { ProvidersTab } from '../components/settings/ProvidersTab';
import { ScopeSelector } from '../components/settings/ScopeSelector';
import { SnapshotsCard } from '../components/settings/SnapshotsCard';
import { VaultTab } from '../components/settings/VaultTab';
import { VectorDbTab } from '../components/settings/VectorDbTab';

export function SettingsPage() {
  const [scope, setScope] = useState<SettingsScopeParam | undefined>(undefined);

  return (
    <>
      <Typography.Title level={4}>Settings</Typography.Title>
      <Card size="small" style={{ marginBottom: 16 }}>
        <ScopeSelector value={scope} onChange={setScope} />
      </Card>
      <SnapshotsCard />
      <Tabs
        defaultActiveKey="presets"
        items={[
          { key: 'presets', label: 'Quick Setup', children: <PresetsCard /> },
          { key: 'vault', label: 'API Keys & Profiles', children: <VaultTab /> },
          { key: 'providers', label: 'Chat & Response Pipeline', children: <ProvidersTab scope={scope} /> },
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
