import { useState } from 'react';
import { Card, Tabs, Tour, Typography } from 'antd';
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
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getSettingsSteps } from '../tours/steps/settings';

export function SettingsPage() {
  const [scope, setScope] = useState<SettingsScopeParam | undefined>(undefined);
  const { t } = useI18n();
  const tour = useTour('settings');

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Typography.Title level={4}>Settings</Typography.Title>
        <TourGuideButton tourId="settings" />
      </div>
      <Card size="small" style={{ marginBottom: 16 }} data-tour="settings-scope">
        <ScopeSelector value={scope} onChange={setScope} />
      </Card>
      <SnapshotsCard />
      <div data-tour="settings-content">
        <Tabs
          data-tour="settings-tabs"
          defaultActiveKey="presets"
          items={[
            { key: 'presets', label: 'Quick Setup', children: <div data-tour="settings-presets"><PresetsCard /></div> },
            { key: 'vault', label: 'API Keys & Profiles', children: <VaultTab /> },
            { key: 'providers', label: 'Chat & Response Pipeline', children: <ProvidersTab scope={scope} /> },
            { key: 'documents', label: 'Document Processing', children: <DocumentProcessingTab /> },
            { key: 'vectordb', label: 'Vector Database', children: <VectorDbTab /> },
            { key: 'prompts', label: 'Agent Prompts', children: <PromptsTab /> },
            { key: 'idp', label: 'Identity Providers', children: <IdpTab /> },
            { key: 'local', label: 'Local Auth', children: <LocalAuthTab /> },
          ]}
        />
      </div>
      <Tour
        open={tour.isActive}
        steps={getSettingsSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
