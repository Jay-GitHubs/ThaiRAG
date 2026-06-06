import { useEffect, useState } from 'react';
import {
  Card,
  Space,
  Typography,
  Table,
  Tag,
  Spin,
  Empty,
  Divider,
  Alert,
  message,
  theme,
} from 'antd';
import { DeploymentUnitOutlined, QuestionCircleOutlined } from '@ant-design/icons';
import { Tooltip } from 'antd';
import {
  getChatPipelineConfig,
  getDocumentConfig,
  getProviderConfig,
  listLlmProfiles,
} from '../../api/settings';
import type {
  ChatPipelineConfigResponse,
  DocumentConfigResponse,
  ProviderConfigResponse,
  LlmProfileInfo,
  LlmProviderInfo,
} from '../../api/types';
import { EmbeddingEditor } from './EmbeddingEditor';
import { VaultTab } from './VaultTab';

const { Text, Title } = Typography;

/** A single resolved consumer: a pipeline slot that points at some LLM config. */
interface Consumer {
  label: string;
  info: LlmProviderInfo;
}

const CHAT_SLOTS: [keyof ChatPipelineConfigResponse, string][] = [
  ['llm', 'Response LLM'],
  ['query_analyzer_llm', 'Query Analyzer'],
  ['query_rewriter_llm', 'Query Rewriter'],
  ['context_curator_llm', 'Context Curator'],
  ['response_generator_llm', 'Response Generator'],
  ['quality_guard_llm', 'Quality Guard'],
  ['language_adapter_llm', 'Language Adapter'],
  ['orchestrator_llm', 'Orchestrator'],
  ['memory_llm', 'Conversation Memory'],
  ['tool_use_llm', 'Tool Use'],
  ['self_rag_llm', 'Self-RAG'],
  ['graph_rag_llm', 'Graph RAG'],
  ['speculative_rag_llm', 'Speculative RAG'],
  ['map_reduce_llm', 'Map-Reduce'],
  ['ragas_llm', 'RAGAS'],
  ['compression_llm', 'Compression'],
  ['multimodal_llm', 'Multimodal'],
  ['raptor_llm', 'RAPTOR'],
  ['colbert_llm', 'ColBERT'],
  ['personal_memory_llm', 'Personal Memory'],
  ['live_retrieval_llm', 'Live Retrieval'],
  ['chat_vision_llm', 'Vision'],
];

const DOC_SLOTS: [string, string][] = [
  ['llm', 'Preprocessing (default)'],
  ['analyzer_llm', 'Analyzer'],
  ['converter_llm', 'Converter'],
  ['quality_llm', 'Quality'],
  ['chunker_llm', 'Chunker'],
  ['orchestrator_llm', 'Orchestrator'],
  ['enricher_llm', 'Enricher'],
];

function collectConsumers(
  chat: ChatPipelineConfigResponse,
  doc: DocumentConfigResponse,
  provider: ProviderConfigResponse,
): Consumer[] {
  const out: Consumer[] = [];
  for (const [key, label] of CHAT_SLOTS) {
    const info = chat[key] as LlmProviderInfo | undefined;
    if (info && info.model) out.push({ label: `Chat · ${label}`, info });
  }
  const ai = doc.ai_preprocessing;
  for (const [key, label] of DOC_SLOTS) {
    const info = (ai as unknown as Record<string, LlmProviderInfo | undefined>)[key];
    if (info && info.model) out.push({ label: `Document · ${label}`, info });
  }
  if (provider.doc_vision_llm && provider.doc_vision_llm.model) {
    out.push({ label: 'Document · Vision', info: provider.doc_vision_llm });
  }
  return out;
}

/**
 * "Used by" map: shows which pipeline slots consume each shared LLM profile, plus
 * the embedding model's global usage. Helps an admin understand the blast radius
 * of editing a shared resource before they change it.
 */
function UsedByMap() {
  const { token } = theme.useToken();
  const [loading, setLoading] = useState(true);
  const [profiles, setProfiles] = useState<LlmProfileInfo[]>([]);
  const [consumers, setConsumers] = useState<Consumer[]>([]);
  const [embedding, setEmbedding] = useState<ProviderConfigResponse['embedding'] | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const [profs, chat, doc, provider] = await Promise.all([
          listLlmProfiles(),
          getChatPipelineConfig(),
          getDocumentConfig(),
          getProviderConfig(),
        ]);
        setProfiles(profs);
        setConsumers(collectConsumers(chat, doc, provider));
        setEmbedding(provider.embedding);
      } catch {
        message.error('Failed to load usage map');
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  if (loading) return <Spin tip="Loading usage map..." />;

  // Profile-backed slots, grouped by profile id.
  const profileRows = profiles.map((p) => {
    const users = consumers.filter((c) => c.info.profile_id === p.id).map((c) => c.label);
    return { key: p.id, name: p.name, kind: p.kind, model: p.model, users };
  });

  // Inline slots (no profile) grouped by model string.
  const inlineByModel = new Map<string, { kind: string; users: string[] }>();
  for (const c of consumers) {
    if (c.info.profile_id) continue;
    const k = `${c.info.kind}/${c.info.model}`;
    const entry = inlineByModel.get(k) || { kind: c.info.kind, users: [] };
    entry.users.push(c.label);
    inlineByModel.set(k, entry);
  }
  const inlineRows = [...inlineByModel.entries()].map(([k, v]) => ({
    key: k,
    model: k.split('/').slice(1).join('/'),
    kind: v.kind,
    users: v.users,
  }));

  const usersColumn = {
    title: 'Used by',
    dataIndex: 'users',
    key: 'users',
    render: (users: string[]) =>
      users.length === 0 ? (
        <Text type="secondary" style={{ fontSize: 12 }}>Not currently used</Text>
      ) : (
        <Space size={[4, 4]} wrap>
          {users.map((u) => (
            <Tag key={u} style={{ fontSize: 11 }}>{u}</Tag>
          ))}
        </Space>
      ),
  };

  return (
    <Card
      title={
        <Space>
          <DeploymentUnitOutlined />
          <span>Used by</span>
          <Tooltip title="Maps each shared resource to the pipeline slots that consume it, so you can see what a change will affect before you make it.">
            <QuestionCircleOutlined style={{ fontSize: 12, color: token.colorTextSecondary }} />
          </Tooltip>
        </Space>
      }
    >
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        {embedding && (
          <Alert
            type="info"
            showIcon
            message={
              <span>
                <Text strong>Embedding</Text> <Tag color="cyan">{embedding.kind}</Tag>
                <Text code>{embedding.model}</Text> (dim {embedding.dimension})
              </span>
            }
            description="Used globally by Chat retrieval (query embedding) and Document ingestion (chunk embedding). Editable above; changing it requires a full re-index."
          />
        )}

        <div>
          <Title level={5} style={{ marginBottom: 8 }}>LLM Profiles</Title>
          {profileRows.length === 0 ? (
            <Empty description="No saved profiles" image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : (
            <Table
              size="small"
              pagination={false}
              dataSource={profileRows}
              columns={[
                {
                  title: 'Profile',
                  dataIndex: 'name',
                  key: 'name',
                  render: (name: string, r) => (
                    <Space>
                      <Text strong>{name}</Text>
                      <Tag>{r.kind}</Tag>
                    </Space>
                  ),
                },
                { title: 'Model', dataIndex: 'model', key: 'model', render: (m: string) => <Text code>{m}</Text> },
                usersColumn,
              ]}
            />
          )}
        </div>

        {inlineRows.length > 0 && (
          <div>
            <Title level={5} style={{ marginBottom: 8 }}>Inline model configs</Title>
            <Text type="secondary" style={{ fontSize: 12 }}>
              Slots configured directly (not via a saved profile).
            </Text>
            <Table
              size="small"
              pagination={false}
              style={{ marginTop: 8 }}
              dataSource={inlineRows}
              columns={[
                {
                  title: 'Model',
                  dataIndex: 'model',
                  key: 'model',
                  render: (m: string, r) => (
                    <Space>
                      <Text code>{m}</Text>
                      <Tag>{r.kind}</Tag>
                    </Space>
                  ),
                },
                usersColumn,
              ]}
            />
          </div>
        )}
      </Space>
    </Card>
  );
}

/**
 * Shared / Common settings tab. Consolidates platform-wide resources that every
 * path depends on: the global embedding model, API keys + LLM profiles (the old
 * "API Keys & Profiles" / Vault tab), and a "Used by" map showing what consumes
 * each shared resource.
 */
export function SharedCommonTab() {
  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <EmbeddingEditor />
      <Divider style={{ margin: 0 }} />
      <VaultTab />
      <Divider style={{ margin: 0 }} />
      <UsedByMap />
    </Space>
  );
}
