import { useEffect, useState } from 'react';
import { Card, InputNumber, Button, Space, Typography, Spin, message, Tooltip } from 'antd';
import { SaveOutlined, QuestionCircleOutlined } from '@ant-design/icons';
import { getSearchConfig, updateSearchConfig } from '../../api/settings';

const { Text, Paragraph } = Typography;

export function RetrievalSettingsCard() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [topK, setTopK] = useState<number | null>(null);
  const [rerankTopK, setRerankTopK] = useState<number | null>(null);

  useEffect(() => {
    let alive = true;
    getSearchConfig()
      .then((cfg) => {
        if (!alive) return;
        setTopK(cfg.top_k);
        setRerankTopK(cfg.rerank_top_k);
      })
      .catch(() => message.error('Failed to load retrieval settings'))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, []);

  const handleSave = async () => {
    if (topK == null || rerankTopK == null) return;
    if (rerankTopK > topK) {
      message.error('Final chunk count (rerank_top_k) must be ≤ candidates (top_k)');
      return;
    }
    setSaving(true);
    try {
      const updated = await updateSearchConfig({ top_k: topK, rerank_top_k: rerankTopK });
      setTopK(updated.top_k);
      setRerankTopK(updated.rerank_top_k);
      message.success('Retrieval settings saved');
    } catch {
      message.error('Failed to save retrieval settings');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card title="Retrieval" size="small">
      <Paragraph type="secondary" style={{ marginTop: 0 }}>
        How many document chunks the hybrid search retrieves and hands to the model when
        composing an answer. Changes take effect immediately (no restart).
      </Paragraph>
      {loading ? (
        <Spin />
      ) : (
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <Space size="large" wrap>
            <div>
              <Text strong>
                Final chunks per answer (rerank_top_k){' '}
                <Tooltip title="The RRF-merged hits are truncated to this count before reranking — this is the number of chunks actually sent to the LLM. This is the knob behind 'chat always returns N chunks'.">
                  <QuestionCircleOutlined />
                </Tooltip>
              </Text>
              <div>
                <InputNumber
                  min={1}
                  max={100}
                  value={rerankTopK ?? undefined}
                  onChange={(v) => setRerankTopK(v ?? null)}
                  style={{ width: 140 }}
                />
              </div>
            </div>
            <div>
              <Text strong>
                Candidates per store (top_k){' '}
                <Tooltip title="How many candidates each of the vector and BM25 stores returns before the RRF merge. Must be ≥ rerank_top_k.">
                  <QuestionCircleOutlined />
                </Tooltip>
              </Text>
              <div>
                <InputNumber
                  min={1}
                  max={200}
                  value={topK ?? undefined}
                  onChange={(v) => setTopK(v ?? null)}
                  style={{ width: 140 }}
                />
              </div>
            </div>
          </Space>
          <Button
            type="primary"
            icon={<SaveOutlined />}
            loading={saving}
            onClick={handleSave}
          >
            Save Retrieval Settings
          </Button>
        </Space>
      )}
    </Card>
  );
}
