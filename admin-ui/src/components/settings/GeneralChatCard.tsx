import { useEffect, useState } from 'react';
import {
  Card,
  Input,
  Button,
  Space,
  Switch,
  Select,
  Typography,
  Spin,
  Divider,
  Tag,
  message,
  Tooltip,
} from 'antd';
import { SaveOutlined, QuestionCircleOutlined } from '@ant-design/icons';
import { getGeneralChatConfig, updateGeneralChatConfig } from '../../api/settings';
import type { UpdateGeneralChatRequest } from '../../api/types';

const { Text, Paragraph } = Typography;

const LLM_KINDS = [
  { label: 'Ollama', value: 'Ollama' },
  { label: 'OpenAI', value: 'OpenAi' },
  { label: 'OpenAI-compatible', value: 'OpenAiCompatible' },
  { label: 'Claude', value: 'Claude' },
  { label: 'Gemini', value: 'Gemini' },
];

/**
 * General (non-RAG) chat settings. Controls whether the chat UI offers a plain
 * assistant mode, the system prompt that frames it, an optional dedicated model
 * (otherwise it reuses the main chat LLM), and capability-gated image
 * generation. Changes take effect immediately — no restart.
 */
export function GeneralChatCard() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const [enabled, setEnabled] = useState(true);
  const [systemPrompt, setSystemPrompt] = useState('');

  // Dedicated model: off = reuse the main chat LLM.
  const [useDedicated, setUseDedicated] = useState(false);
  const [hasApiKey, setHasApiKey] = useState(false);
  const [llmKind, setLlmKind] = useState('OpenAiCompatible');
  const [llmModel, setLlmModel] = useState('');
  const [llmBaseUrl, setLlmBaseUrl] = useState('');
  const [llmApiKey, setLlmApiKey] = useState('');

  const [imgEnabled, setImgEnabled] = useState(false);
  const [imgModel, setImgModel] = useState('');
  const [imgBaseUrl, setImgBaseUrl] = useState('');

  useEffect(() => {
    let alive = true;
    getGeneralChatConfig()
      .then((cfg) => {
        if (!alive) return;
        setEnabled(cfg.enabled);
        setSystemPrompt(cfg.system_prompt);
        setUseDedicated(cfg.llm != null);
        if (cfg.llm) {
          setLlmKind(cfg.llm.kind);
          setLlmModel(cfg.llm.model);
          setLlmBaseUrl(cfg.llm.base_url ?? '');
          setHasApiKey(cfg.llm.has_api_key);
        }
        setImgEnabled(cfg.image_generation.enabled);
        setImgModel(cfg.image_generation.model);
        setImgBaseUrl(cfg.image_generation.base_url);
      })
      .catch(() => message.error('Failed to load general chat settings'))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, []);

  const handleSave = async () => {
    if (systemPrompt.trim().length === 0) {
      message.error('System prompt cannot be empty');
      return;
    }
    if (useDedicated && llmModel.trim().length === 0) {
      message.error('A dedicated model needs a model name');
      return;
    }
    if (imgEnabled && imgModel.trim().length === 0) {
      message.error('Image generation needs a model name to enable');
      return;
    }

    const payload: UpdateGeneralChatRequest = {
      enabled,
      system_prompt: systemPrompt,
      image_generation: { enabled: imgEnabled, model: imgModel, base_url: imgBaseUrl },
    };
    if (!useDedicated) {
      payload.clear_llm = true;
    } else {
      payload.llm = {
        kind: llmKind,
        model: llmModel,
        base_url: llmBaseUrl,
        // Only send the key when the operator typed a new one.
        ...(llmApiKey ? { api_key: llmApiKey } : {}),
      };
    }

    setSaving(true);
    try {
      const updated = await updateGeneralChatConfig(payload);
      setEnabled(updated.enabled);
      setSystemPrompt(updated.system_prompt);
      setUseDedicated(updated.llm != null);
      setHasApiKey(updated.llm?.has_api_key ?? false);
      setLlmApiKey('');
      setImgEnabled(updated.image_generation.enabled);
      message.success('General chat settings saved');
    } catch {
      message.error('Failed to save general chat settings');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card title="General Chat (non-RAG)" size="small">
      <Paragraph type="secondary" style={{ marginTop: 0 }}>
        A plain-assistant mode the chat UI offers alongside knowledge-base search. In General mode
        the agent answers from the model’s own knowledge and never retrieves from your corpus, so
        users see plainly why it isn’t citing their documents. Changes take effect immediately.
      </Paragraph>
      {loading ? (
        <Spin />
      ) : (
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          <Space size="middle">
            <Switch data-testid="gc-enabled" checked={enabled} onChange={setEnabled} />
            <Text strong>Offer General mode in the chat UI</Text>
          </Space>

          <div>
            <Text strong>System prompt</Text>
            <Input.TextArea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              autoSize={{ minRows: 2, maxRows: 6 }}
              placeholder="You are a helpful general-purpose assistant…"
              style={{ marginTop: 6 }}
            />
          </div>

          <Divider style={{ margin: '4px 0' }} />

          <Space size="middle">
            <Switch data-testid="gc-dedicated" checked={useDedicated} onChange={setUseDedicated} />
            <Text strong>
              Use a dedicated model{' '}
              <Tooltip title="Off (default): general chat reuses the main chat LLM. On: route general chat to a separate provider/model.">
                <QuestionCircleOutlined />
              </Tooltip>
            </Text>
            {!useDedicated && <Tag color="default">reuses main chat LLM</Tag>}
          </Space>

          {useDedicated && (
            <Space direction="vertical" size="middle" style={{ width: '100%' }}>
              <Space size="large" wrap>
                <div>
                  <Text type="secondary">Provider</Text>
                  <div>
                    <Select
                      value={llmKind}
                      onChange={setLlmKind}
                      options={LLM_KINDS}
                      style={{ width: 200 }}
                    />
                  </div>
                </div>
                <div>
                  <Text type="secondary">Model</Text>
                  <div>
                    <Input
                      value={llmModel}
                      onChange={(e) => setLlmModel(e.target.value)}
                      placeholder="e.g. qwen-235b"
                      style={{ width: 240 }}
                    />
                  </div>
                </div>
              </Space>
              <div>
                <Text type="secondary">Base URL</Text>
                <Input
                  value={llmBaseUrl}
                  onChange={(e) => setLlmBaseUrl(e.target.value)}
                  placeholder="https://llm.example.com/v1"
                />
              </div>
              <div>
                <Text type="secondary">API key</Text>
                <Input.Password
                  value={llmApiKey}
                  onChange={(e) => setLlmApiKey(e.target.value)}
                  placeholder={hasApiKey ? '•••••••• (unchanged)' : 'sk-…'}
                  autoComplete="new-password"
                />
              </div>
            </Space>
          )}

          <Divider style={{ margin: '4px 0' }} />

          <Space size="middle">
            <Switch data-testid="gc-image" checked={imgEnabled} onChange={setImgEnabled} />
            <Text strong>
              Image generation{' '}
              <Tooltip title="Lets General mode generate images via an OpenAI-compatible /images/generations endpoint. Requires a text-to-image model — leave off if your provider has none.">
                <QuestionCircleOutlined />
              </Tooltip>
            </Text>
          </Space>

          {imgEnabled && (
            <Space size="large" wrap>
              <div>
                <Text type="secondary">Image model</Text>
                <div>
                  <Input
                    value={imgModel}
                    onChange={(e) => setImgModel(e.target.value)}
                    placeholder="e.g. dall-e-3"
                    style={{ width: 240 }}
                  />
                </div>
              </div>
              <div>
                <Text type="secondary">
                  Base URL{' '}
                  <Tooltip title="Optional. Defaults to the dedicated/main LLM base URL if left blank.">
                    <QuestionCircleOutlined />
                  </Tooltip>
                </Text>
                <div>
                  <Input
                    value={imgBaseUrl}
                    onChange={(e) => setImgBaseUrl(e.target.value)}
                    placeholder="(inherit LLM base URL)"
                    style={{ width: 280 }}
                  />
                </div>
              </div>
            </Space>
          )}

          <Button type="primary" icon={<SaveOutlined />} loading={saving} onClick={handleSave}>
            Save General Chat Settings
          </Button>
        </Space>
      )}
    </Card>
  );
}
