import { useState, useRef, useEffect } from 'react';
import {
  Typography,
  Select,
  Breadcrumb,
  Space,
  Input,
  Button,
  Card,
  Spin,
  Tag,
  Collapse,
  Empty,
  Tooltip,
  Modal,
  message,
  theme,
  Tour,
} from 'antd';
import {
  SendOutlined,
  FileTextOutlined,
  PaperClipOutlined,
  ClearOutlined,
  ClockCircleOutlined,
  SearchOutlined,
  RobotOutlined,
  QuestionCircleOutlined,
  ThunderboltOutlined,
  LoadingOutlined,
  LikeOutlined,
  LikeFilled,
  DislikeOutlined,
  DislikeFilled,
  StarOutlined,
  FieldTimeOutlined,
  CheckCircleOutlined,
  MinusCircleOutlined,
  ExclamationCircleOutlined,
  DashboardOutlined,
} from '@ant-design/icons';
import { useOrgs } from '../hooks/useOrgs';
import { useDepts } from '../hooks/useDepts';
import { useWorkspaces } from '../hooks/useWorkspaces';
import { testQueryStream } from '../api/testQuery';
import {
  fileToAttachment,
  ACCEPTED_EXTENSIONS,
  MAX_ATTACHMENT_BYTES,
  MAX_ATTACHMENTS,
  type Attachment,
} from '../api/attachments';
import { submitFeedback } from '../api/feedback';
import type { RetrievedChunk, TestQueryUsage, TestQueryTiming, TestQueryProviderInfo, PipelineStage, PipelineProgress } from '../api/types';
import { useI18n } from '../i18n';
import { useTour, TourGuideButton } from '../tours';
import { getTestChatSteps } from '../tours/steps/testChat';

interface ChatEntry {
  role: 'user' | 'assistant';
  content: string;
  responseId?: string;
  chunks?: RetrievedChunk[];
  usage?: TestQueryUsage;
  timing?: TestQueryTiming;
  providerInfo?: TestQueryProviderInfo;
  pipelineStages?: PipelineStage[];
  feedback?: 'up' | 'down';
  query?: string;
}

const TIMEOUT_PRESETS = [
  { label: '30 seconds', value: 30_000 },
  { label: '1 minute', value: 60_000 },
  { label: '2 minutes', value: 120_000 },
  { label: '5 minutes', value: 300_000 },
  { label: '10 minutes', value: 600_000 },
  { label: 'No limit', value: 0 },
];

const TIMEOUT_STORAGE_KEY = 'thairag-test-chat-timeout';
const CHAT_STORAGE_KEY = 'thairag-test-chat-history';
const SELECTION_STORAGE_KEY = 'thairag-test-chat-selection';

function loadSavedTimeout(): number {
  const saved = localStorage.getItem(TIMEOUT_STORAGE_KEY);
  return saved ? Number(saved) : 120_000;
}

function loadSavedChat(): ChatEntry[] {
  try {
    const saved = sessionStorage.getItem(CHAT_STORAGE_KEY);
    return saved ? JSON.parse(saved) : [];
  } catch { return []; }
}

function saveChatHistory(messages: ChatEntry[]) {
  try {
    sessionStorage.setItem(CHAT_STORAGE_KEY, JSON.stringify(messages));
  } catch { /* quota exceeded — ignore */ }
}

function loadSavedSelection(): { orgId?: string; deptId?: string; wsId?: string } {
  try {
    const saved = sessionStorage.getItem(SELECTION_STORAGE_KEY);
    return saved ? JSON.parse(saved) : {};
  } catch { return {}; }
}

function saveSelection(orgId?: string, deptId?: string, wsId?: string) {
  sessionStorage.setItem(SELECTION_STORAGE_KEY, JSON.stringify({ orgId, deptId, wsId }));
}

export function TestChatPage() {
  const savedSelection = loadSavedSelection();
  const [orgId, setOrgId] = useState<string | undefined>(savedSelection.orgId);
  const [deptId, setDeptId] = useState<string | undefined>(savedSelection.deptId);
  const [wsId, setWsId] = useState<string | undefined>(savedSelection.wsId);
  const [query, setQuery] = useState('');
  const [messages, setMessages] = useState<ChatEntry[]>(loadSavedChat);
  const [loading, setLoading] = useState(false);
  const [liveStages, setLiveStages] = useState<PipelineProgress[]>([]);
  const liveStagesRef = useRef<PipelineProgress[]>([]);
  const [timeoutMs, setTimeoutMs] = useState(loadSavedTimeout);
  const [commentModal, setCommentModal] = useState<{ index: number; thumbsUp: boolean } | null>(null);
  const [commentText, setCommentText] = useState('');
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const { token: themeToken } = theme.useToken();
  const { t } = useI18n();
  const tour = useTour('test-chat');

  const handleTimeoutChange = (value: number) => {
    setTimeoutMs(value);
    localStorage.setItem(TIMEOUT_STORAGE_KEY, String(value));
  };

  const orgs = useOrgs();
  const depts = useDepts(orgId);
  const workspaces = useWorkspaces(orgId, deptId);

  const orgName = orgs.data?.data.find((o) => o.id === orgId)?.name;
  const deptName = depts.data?.data.find((d) => d.id === deptId)?.name;
  const wsName = workspaces.data?.data.find((w) => w.id === wsId)?.name;

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    saveChatHistory(messages);
  }, [messages]);

  useEffect(() => {
    saveSelection(orgId, deptId, wsId);
  }, [orgId, deptId, wsId]);

  const [sseBuffered, setSseBuffered] = useState(false);

  const handleSend = async () => {
    const q = query.trim();
    if (!q || !wsId || loading) return;

    setMessages((prev) => [...prev, { role: 'user', content: q }]);
    setQuery('');
    setLoading(true);
    setLiveStages([]);
    liveStagesRef.current = [];
    setSseBuffered(false);

    const abortController = new AbortController();
    if (timeoutMs > 0) {
      setTimeout(() => abortController.abort(), timeoutMs);
    }

    // Detect if SSE events are being buffered by a proxy (Kong, Cloudflare, etc.)
    // If no progress event arrives within 3s, assume buffering and show fallback UI.
    let gotFirstEvent = false;
    const bufferDetector = setTimeout(() => {
      if (!gotFirstEvent) setSseBuffered(true);
    }, 3000);

    try {
      const res = await testQueryStream(
        wsId,
        q,
        (progress: PipelineProgress) => {
          gotFirstEvent = true;
          clearTimeout(bufferDetector);
          if (sseBuffered) setSseBuffered(false);

          // Update ref IMMEDIATELY (outside React's batched state updater)
          // so it's available when testQueryStream resolves.
          const prev = liveStagesRef.current;
          const idx = prev.findIndex((s) => s.stage === progress.stage && s.status === 'started');
          let next: PipelineProgress[];
          if (progress.status !== 'started' && idx >= 0) {
            next = [...prev];
            next[idx] = { ...progress, model: progress.model || prev[idx].model };
          } else {
            next = [...prev, progress];
          }
          liveStagesRef.current = next;
          setLiveStages(next);
        },
        abortController.signal,
        attachments,
      );
      clearTimeout(bufferDetector);

      // Build pipeline stages from streamed events (server sends empty array for streaming)
      const collectedStages: PipelineStage[] = liveStagesRef.current
        .filter((s) => s.status !== 'started')
        .map((s) => ({
          stage: s.stage,
          status: s.status as 'done' | 'skipped' | 'error',
          duration_ms: s.duration_ms,
          model: s.model,
        }));

      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: res.answer,
          responseId: res.response_id,
          chunks: res.chunks,
          usage: res.usage,
          timing: res.timing,
          providerInfo: res.provider_info,
          pipelineStages: collectedStages.length > 0 ? collectedStages : res.pipeline_stages,
          query: q,
        },
      ]);
    } catch (err: unknown) {
      const isAbort = err instanceof DOMException && err.name === 'AbortError';
      const msg = isAbort
        ? `Request timed out after ${timeoutMs ? (timeoutMs / 1000) + 's' : 'default timeout'}. Try increasing the timeout.`
        : err instanceof Error ? err.message : 'Failed to get response';
      setMessages((prev) => [
        ...prev,
        { role: 'assistant', content: `Error: ${msg}` },
      ]);
    } finally {
      setLoading(false);
      setLiveStages([]);
    }
  };

  const handleFeedback = async (index: number, thumbsUp: boolean) => {
    const entry = messages[index];
    if (!entry.responseId) return;

    // If clicking the same feedback, un-toggle
    if (entry.feedback === (thumbsUp ? 'up' : 'down')) {
      setMessages((prev) => {
        const updated = [...prev];
        updated[index] = { ...updated[index], feedback: undefined };
        return updated;
      });
      return;
    }

    // For negative feedback, show comment modal
    if (!thumbsUp) {
      setCommentModal({ index, thumbsUp });
      setCommentText('');
      return;
    }

    // Positive feedback — send immediately
    await sendFeedback(index, thumbsUp, undefined);
  };

  const sendFeedback = async (index: number, thumbsUp: boolean, comment?: string) => {
    const entry = messages[index];
    if (!entry.responseId) return;

    try {
      await submitFeedback({
        response_id: entry.responseId,
        thumbs_up: thumbsUp,
        comment,
        query: entry.query,
        answer: entry.content,
        workspace_id: wsId,
        doc_ids: entry.chunks?.map((c) => c.doc_id) ?? [],
        chunk_scores: entry.chunks?.map((c) => c.score) ?? [],
        chunk_ids: entry.chunks?.map((c) => c.chunk_id) ?? [],
      });

      setMessages((prev) => {
        const updated = [...prev];
        updated[index] = { ...updated[index], feedback: thumbsUp ? 'up' : 'down' };
        return updated;
      });

      message.success(thumbsUp ? 'Thanks for the positive feedback!' : 'Feedback recorded. We\'ll work on improving.');
    } catch {
      message.error('Failed to submit feedback');
    }
  };

  const handleCommentSubmit = async () => {
    if (!commentModal) return;
    await sendFeedback(commentModal.index, commentModal.thumbsUp, commentText || undefined);
    setCommentModal(null);
    setCommentText('');
  };

  const handleClear = () => {
    setMessages([]);
    setAttachments([]);
    sessionStorage.removeItem(CHAT_STORAGE_KEY);
  };

  const handleFilesPicked = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    e.target.value = ''; // reset so the same file can be re-picked
    if (files.length === 0) return;

    const room = MAX_ATTACHMENTS - attachments.length;
    if (room <= 0) {
      message.warning(`Maximum ${MAX_ATTACHMENTS} attachments.`);
      return;
    }
    const picked = files.slice(0, room);
    if (files.length > room) {
      message.warning(`Only ${room} more attachment(s) allowed — extra files ignored.`);
    }

    const tooBig = picked.filter((f) => f.size > MAX_ATTACHMENT_BYTES);
    if (tooBig.length > 0) {
      message.error(`${tooBig.map((f) => f.name).join(', ')} exceed the 5 MB limit.`);
    }
    const withinLimit = picked.filter((f) => f.size <= MAX_ATTACHMENT_BYTES);

    try {
      const encoded = await Promise.all(withinLimit.map(fileToAttachment));
      setAttachments((prev) => [...prev, ...encoded]);
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Failed to read file');
    }
  };

  const removeAttachment = (index: number) => {
    setAttachments((prev) => prev.filter((_, i) => i !== index));
  };

  const scoreColor = (score: number) => {
    if (score >= 0.02) return 'green';
    if (score >= 0.01) return 'blue';
    if (score >= 0.005) return 'orange';
    return 'default';
  };

  const formatScore = (score: number) => score.toFixed(4);

  const STAGE_INFO: Record<string, { name: string; task: string }> = {
    query_analyzer: { name: 'Query Analyzer', task: 'Analyzing intent, language & complexity' },
    self_rag_gate: { name: 'Self-RAG Gate', task: 'Deciding whether retrieval is needed' },
    pipeline_orchestrator: { name: 'Pipeline Orchestrator', task: 'Choosing the optimal pipeline route' },
    query_rewriter: { name: 'Query Rewriter', task: 'Rewriting query for better retrieval' },
    search: { name: 'Hybrid Search', task: 'Searching vector store & BM25 index' },
    colbert_reranker: { name: 'ColBERT Reranker', task: 'Re-ranking results with ColBERT' },
    graph_rag: { name: 'Graph RAG', task: 'Extracting entities & traversing knowledge graph' },
    context_curator: { name: 'Context Curator', task: 'Scoring & selecting the best context' },
    retrieval_refinement: { name: 'Retrieval Refinement', task: 'Refining retrieval with feedback signals' },
    corrective_rag: { name: 'Corrective RAG', task: 'Checking & correcting retrieved context' },
    live_retrieval: { name: 'Live Source Retrieval', task: 'Fetching from external sources (OneDrive, web, etc.)' },
    raptor: { name: 'RAPTOR', task: 'Building hierarchical document summaries' },
    contextual_compression: { name: 'Contextual Compression', task: 'Compressing context to key information' },
    multimodal_rag: { name: 'Multi-modal RAG', task: 'Processing images & tables from documents' },
    map_reduce: { name: 'Map-Reduce', task: 'Summarizing chunks in parallel' },
    response_generator: { name: 'Response Generator', task: 'Generating the final answer' },
    quality_guard: { name: 'Quality Guard', task: 'Checking answer quality & hallucinations' },
    language_adapter: { name: 'Language Adapter', task: 'Adapting response language to match query' },
  };

  const formatStageName = (stage: string) =>
    STAGE_INFO[stage]?.name ?? stage.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());

  const formatStageTask = (stage: string) =>
    STAGE_INFO[stage]?.task ?? '';

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>Test KM Chat</Typography.Title>
        <Tooltip title="Test your knowledge base by asking questions. The system searches for relevant document chunks, then generates an answer using your configured LLM. Use thumbs up/down to rate answers — this feedback auto-tunes the system over time.">
          <QuestionCircleOutlined style={{ color: themeToken.colorTextSecondary, fontSize: 16 }} />
        </Tooltip>
        <TourGuideButton tourId="test-chat" />
      </div>

      <Space style={{ marginBottom: 16 }} wrap data-tour="chat-ws-select">
        <Select
          placeholder="Select Organization"
          style={{ width: 200 }}
          value={orgId}
          onChange={(v) => {
            setOrgId(v);
            setDeptId(undefined);
            setWsId(undefined);
            setMessages([]);
          }}
          options={orgs.data?.data.map((o) => ({ label: o.name, value: o.id }))}
          allowClear
        />
        <Select
          placeholder="Select Department"
          style={{ width: 200 }}
          value={deptId}
          onChange={(v) => {
            setDeptId(v);
            setWsId(undefined);
            setMessages([]);
          }}
          options={depts.data?.data.map((d) => ({ label: d.name, value: d.id }))}
          disabled={!orgId}
          allowClear
        />
        <Select
          placeholder="Select Workspace"
          style={{ width: 200 }}
          value={wsId}
          onChange={(v) => {
            setWsId(v);
            setMessages([]);
          }}
          options={workspaces.data?.data.map((w) => ({ label: w.name, value: w.id }))}
          disabled={!deptId}
          allowClear
        />
      </Space>

      {orgName && (
        <Breadcrumb
          style={{ marginBottom: 16 }}
          items={[
            { title: orgName },
            ...(deptName ? [{ title: deptName }] : []),
            ...(wsName ? [{ title: wsName }] : []),
          ]}
        />
      )}

      {wsId ? (
        <div style={{ display: 'flex', flexDirection: 'column', height: 'calc(100vh - 280px)' }}>
          {/* Messages area */}
          <div
            data-tour="chat-response"
            style={{
              flex: 1,
              overflowY: 'auto',
              padding: 16,
              border: `1px solid ${themeToken.colorBorderSecondary}`,
              borderRadius: themeToken.borderRadius,
              marginBottom: 12,
              background: themeToken.colorBgContainer,
            }}
          >
            {messages.length === 0 && (
              <Empty
                description={
                  <span>
                    Ask a question to test the knowledge base in{' '}
                    <strong>{wsName}</strong>
                  </span>
                }
              />
            )}

            {messages.map((msg, i) => (
              <div
                key={i}
                style={{
                  marginBottom: 16,
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: msg.role === 'user' ? 'flex-end' : 'flex-start',
                }}
              >
                <Card
                  size="small"
                  style={{
                    maxWidth: '80%',
                    background:
                      msg.role === 'user'
                        ? themeToken.colorPrimaryBg
                        : themeToken.colorBgElevated,
                  }}
                >
                  <div style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                    {msg.content}
                  </div>
                </Card>

                {/* Feedback buttons + Timing */}
                {msg.role === 'assistant' && msg.responseId && (
                  <div style={{ maxWidth: '80%', marginTop: 6 }}>
                    <Space size={8} wrap style={{ fontSize: 12 }}>
                      {/* Thumbs up/down */}
                      <Tooltip title="Good answer — helps improve future responses">
                        <Button
                          type="text"
                          size="small"
                          icon={msg.feedback === 'up' ? <LikeFilled style={{ color: themeToken.colorSuccess }} /> : <LikeOutlined />}
                          onClick={() => handleFeedback(i, true)}
                        />
                      </Tooltip>
                      <Tooltip title="Bad answer — tell us what went wrong">
                        <Button
                          type="text"
                          size="small"
                          icon={msg.feedback === 'down' ? <DislikeFilled style={{ color: themeToken.colorError }} /> : <DislikeOutlined />}
                          onClick={() => handleFeedback(i, false)}
                        />
                      </Tooltip>
                      <Tooltip title="Save as golden example — this Q&A pair will be used as a few-shot example in future prompts">
                        <Button
                          type="text"
                          size="small"
                          icon={<StarOutlined />}
                          onClick={async () => {
                            try {
                              const { createGoldenExample } = await import('../api/settings');
                              await createGoldenExample({
                                response_id: msg.responseId,
                                query: msg.query ?? '',
                                answer: msg.content,
                                workspace_id: wsId,
                              });
                              message.success('Saved as golden example');
                            } catch {
                              message.error('Failed to save golden example');
                            }
                          }}
                        />
                      </Tooltip>

                      {/* Timing tags */}
                      {msg.timing && (
                        <>
                          <Tooltip title="Total time from request to response">
                            <Tag icon={<ClockCircleOutlined />}>
                              Total: {msg.timing.total_ms}ms
                            </Tag>
                          </Tooltip>
                          <Tooltip title="Time to search vector store + BM25 and retrieve relevant chunks">
                            <Tag icon={<SearchOutlined />} color="blue">
                              Search: {msg.timing.search_ms}ms
                            </Tag>
                          </Tooltip>
                          <Tooltip title="Time for the LLM to generate the answer (includes all pipeline agents)">
                            <Tag icon={<RobotOutlined />} color="purple">
                              Generation: {msg.timing.generation_ms}ms
                            </Tag>
                          </Tooltip>
                        </>
                      )}
                      {msg.providerInfo && (
                        <Tooltip title={`LLM: ${msg.providerInfo.llm_kind} / ${msg.providerInfo.llm_model}\nEmbedding: ${msg.providerInfo.embedding_kind} / ${msg.providerInfo.embedding_model}`}>
                          <Tag icon={<ThunderboltOutlined />} color="cyan">
                            {msg.providerInfo.llm_model}
                          </Tag>
                        </Tooltip>
                      )}
                    </Space>
                  </div>
                )}

                {/* Pipeline stages */}
                {msg.pipelineStages && msg.pipelineStages.length > 0 && (
                  <div data-tour="chat-pipeline" style={{ maxWidth: '80%', marginTop: 8 }}>
                    <Collapse
                      size="small"
                      items={[
                        {
                          key: 'pipeline',
                          label: (
                            <Space>
                              <DashboardOutlined />
                              <span>
                                Pipeline Stages ({msg.pipelineStages.length})
                              </span>
                              {(() => {
                                const totalMs = msg.pipelineStages
                                  .filter((s) => s.status === 'done' && s.duration_ms != null)
                                  .reduce((sum, s) => sum + (s.duration_ms ?? 0), 0);
                                const slowest = msg.pipelineStages
                                  .filter((s) => s.status === 'done' && s.duration_ms != null)
                                  .sort((a, b) => (b.duration_ms ?? 0) - (a.duration_ms ?? 0))[0];
                                return (
                                  <>
                                    <Tag color="blue">{totalMs.toLocaleString()}ms total</Tag>
                                    {slowest && (slowest.duration_ms ?? 0) > 1000 && (
                                      <Tooltip title={`Slowest stage: ${slowest.stage} took ${(slowest.duration_ms ?? 0).toLocaleString()}ms`}>
                                        <Tag color="orange">
                                          Bottleneck: {formatStageName(slowest.stage)}
                                        </Tag>
                                      </Tooltip>
                                    )}
                                  </>
                                );
                              })()}
                            </Space>
                          ),
                          children: (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                              {msg.pipelineStages.map((stage, si) => {
                                const durationMs = stage.duration_ms ?? 0;
                                const isSlow = stage.status === 'done' && durationMs > 2000;
                                const isVerySlow = stage.status === 'done' && durationMs > 5000;
                                return (
                                  <div
                                    key={si}
                                    style={{
                                      display: 'flex',
                                      alignItems: 'center',
                                      gap: 8,
                                      padding: '4px 8px',
                                      borderRadius: 4,
                                      background: isVerySlow
                                        ? themeToken.colorErrorBg
                                        : isSlow
                                          ? themeToken.colorWarningBg
                                          : 'transparent',
                                    }}
                                  >
                                    {stage.status === 'done' && (
                                      <CheckCircleOutlined style={{ color: themeToken.colorSuccess }} />
                                    )}
                                    {stage.status === 'skipped' && (
                                      <MinusCircleOutlined style={{ color: themeToken.colorTextQuaternary }} />
                                    )}
                                    {stage.status === 'error' && (
                                      <ExclamationCircleOutlined style={{ color: themeToken.colorError }} />
                                    )}
                                    <span style={{
                                      fontSize: 13,
                                      color: stage.status === 'skipped'
                                        ? themeToken.colorTextQuaternary
                                        : themeToken.colorText,
                                    }}>
                                      {formatStageName(stage.stage)}
                                    </span>
                                    {stage.model && (
                                      <Tag style={{ fontSize: 11, lineHeight: '18px', margin: 0 }}>{stage.model}</Tag>
                                    )}
                                    <span style={{ flex: 1 }} />
                                    {stage.status === 'done' && stage.duration_ms != null && (
                                      <Tag
                                        color={isVerySlow ? 'error' : isSlow ? 'warning' : 'default'}
                                        style={{ margin: 0, fontVariantNumeric: 'tabular-nums' }}
                                      >
                                        {stage.duration_ms.toLocaleString()}ms
                                      </Tag>
                                    )}
                                    {stage.status === 'skipped' && (
                                      <span style={{ fontSize: 12, color: themeToken.colorTextQuaternary }}>
                                        skipped
                                      </span>
                                    )}
                                  </div>
                                );
                              })}
                            </div>
                          ),
                        },
                      ]}
                    />
                  </div>
                )}

                {/* Retrieved chunks */}
                {msg.chunks && msg.chunks.length > 0 && (
                  <div style={{ maxWidth: '80%', marginTop: 8 }}>
                    <Collapse
                      size="small"
                      items={[
                        {
                          key: 'chunks',
                          label: (
                            <Space>
                              <FileTextOutlined />
                              <span>
                                {msg.chunks.length} chunk{msg.chunks.length > 1 ? 's' : ''} retrieved
                              </span>
                              <Tooltip title="These are the document segments retrieved from the vector store that were used as context for the LLM. Expand to inspect content quality, relevance scores, and source documents.">
                                <QuestionCircleOutlined style={{ color: themeToken.colorTextSecondary }} />
                              </Tooltip>
                            </Space>
                          ),
                          children: (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                              {msg.chunks.map((chunk, ci) => (
                                <Card
                                  key={ci}
                                  size="small"
                                  title={
                                    <Space size={4} wrap>
                                      <Tooltip title="RRF (Reciprocal Rank Fusion) score combining vector similarity and BM25 text search. Higher = more relevant. Typical range: 0.005–0.05.">
                                        <Tag color={scoreColor(chunk.score)}>
                                          score: {formatScore(chunk.score)}
                                        </Tag>
                                      </Tooltip>
                                      {chunk.doc_title && (
                                        <Tooltip title={`Document ID: ${chunk.doc_id}`}>
                                          <Tag icon={<FileTextOutlined />}>
                                            {chunk.doc_title}
                                          </Tag>
                                        </Tooltip>
                                      )}
                                      {chunk.page_numbers && chunk.page_numbers.length > 0 && (
                                        <Tooltip title="Page number(s) in the original document where this chunk was extracted from">
                                          <Tag>p.{chunk.page_numbers.join(',')}</Tag>
                                        </Tooltip>
                                      )}
                                      {chunk.section_title && (
                                        <Tooltip title="Section heading detected in the document">
                                          <Tag>{chunk.section_title}</Tag>
                                        </Tooltip>
                                      )}
                                      <Tooltip title="Chunk index — the sequential position of this text segment within the document. #0 is the first chunk, #1 the second, etc.">
                                        <Tag>#{chunk.chunk_index}</Tag>
                                      </Tooltip>
                                    </Space>
                                  }
                                >
                                  <div
                                    style={{
                                      whiteSpace: 'pre-wrap',
                                      wordBreak: 'break-word',
                                      fontSize: 12,
                                      maxHeight: 200,
                                      overflowY: 'auto',
                                      fontFamily: 'monospace',
                                      color: themeToken.colorText,
                                      background: themeToken.colorBgLayout,
                                      padding: 8,
                                      borderRadius: 4,
                                    }}
                                  >
                                    {chunk.content}
                                  </div>
                                </Card>
                              ))}
                            </div>
                          ),
                        },
                      ]}
                    />

                    {/* Token usage */}
                    {msg.usage && (
                      <div style={{ marginTop: 6 }}>
                        <Space size={8} wrap style={{ fontSize: 12 }}>
                          <Tooltip title="Total LLM tokens consumed for this query (prompt tokens sent to the model + completion tokens generated)">
                            <span style={{ color: themeToken.colorTextSecondary }}>
                              Tokens: {msg.usage.total_tokens.toLocaleString()} (prompt: {msg.usage.prompt_tokens.toLocaleString()}, completion: {msg.usage.completion_tokens.toLocaleString()})
                            </span>
                          </Tooltip>
                        </Space>
                      </div>
                    )}
                  </div>
                )}
              </div>
            ))}

            {loading && (
              <div style={{ padding: '12px 16px' }}>
                {liveStages.length === 0 && !sseBuffered ? (
                  <div style={{ textAlign: 'center' }}>
                    <Spin tip="Connecting..." />
                  </div>
                ) : liveStages.length === 0 && sseBuffered ? (
                  <Card size="small" style={{ maxWidth: '80%' }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '8px 0' }}>
                      <LoadingOutlined spin style={{ fontSize: 20, color: themeToken.colorPrimary }} />
                      <div>
                        <div style={{ fontWeight: 500 }}>Processing your query...</div>
                        <div style={{ fontSize: 12, color: themeToken.colorTextSecondary, marginTop: 2 }}>
                          Analyzing, searching, and generating answer. Pipeline stages will appear after completion.
                        </div>
                      </div>
                    </div>
                  </Card>
                ) : (
                  <Card size="small" style={{ maxWidth: '80%' }}>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                      {liveStages
                        .filter((s, i, arr) => {
                          // Show only the latest event per stage
                          for (let j = arr.length - 1; j >= 0; j--) {
                            if (arr[j].stage === s.stage) return i === j;
                          }
                          return true;
                        })
                        .map((s) => {
                          const isActive = s.status === 'started';
                          const isDone = s.status === 'done';
                          const isSkipped = s.status === 'skipped';
                          const icon =
                            isActive ? <LoadingOutlined spin style={{ color: themeToken.colorPrimary }} /> :
                            isDone ? <CheckCircleOutlined style={{ color: themeToken.colorSuccess }} /> :
                            isSkipped ? <MinusCircleOutlined style={{ color: themeToken.colorTextQuaternary }} /> :
                            <ExclamationCircleOutlined style={{ color: themeToken.colorError }} />;
                          const taskDesc = formatStageTask(s.stage);
                          return (
                            <div
                              key={s.stage}
                              style={{
                                display: 'flex',
                                alignItems: 'center',
                                gap: 8,
                                fontSize: 13,
                                padding: '3px 6px',
                                borderRadius: 4,
                                background: isActive ? themeToken.colorPrimaryBg : 'transparent',
                                transition: 'background 0.2s',
                              }}
                            >
                              {icon}
                              <span style={{
                                fontWeight: isActive ? 600 : 400,
                                color: isActive
                                  ? themeToken.colorPrimaryText
                                  : isSkipped
                                    ? themeToken.colorTextQuaternary
                                    : themeToken.colorTextSecondary,
                              }}>
                                {formatStageName(s.stage)}
                              </span>
                              {isActive && taskDesc && (
                                <span style={{ color: themeToken.colorTextSecondary, fontSize: 12, fontStyle: 'italic' }}>
                                  — {taskDesc}
                                </span>
                              )}
                              {s.model && (
                                <Tag style={{ fontSize: 11, lineHeight: '18px', margin: 0 }}>{s.model}</Tag>
                              )}
                              <span style={{ flex: 1 }} />
                              {isDone && s.duration_ms != null && (
                                <span style={{ color: themeToken.colorTextQuaternary, fontSize: 12, fontVariantNumeric: 'tabular-nums' }}>
                                  {s.duration_ms < 1000 ? `${s.duration_ms}ms` : `${(s.duration_ms / 1000).toFixed(1)}s`}
                                </span>
                              )}
                              {isSkipped && (
                                <span style={{ color: themeToken.colorTextQuaternary, fontSize: 12 }}>skipped</span>
                              )}
                            </div>
                          );
                        })}
                    </div>
                  </Card>
                )}
              </div>
            )}

            <div ref={messagesEndRef} />
          </div>

          {/* Attachment chips — files attached for the next query */}
          {attachments.length > 0 && (
            <div
              style={{
                marginBottom: 8,
                display: 'flex',
                flexWrap: 'wrap',
                gap: 4,
                alignItems: 'center',
              }}
            >
              <Tooltip title="When documents are attached, the workspace search is skipped and answers come directly from the attached files.">
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  <PaperClipOutlined /> Attached (search skipped):
                </Typography.Text>
              </Tooltip>
              {attachments.map((a, i) => (
                <Tag
                  key={i}
                  icon={<FileTextOutlined />}
                  closable
                  onClose={() => removeAttachment(i)}
                >
                  {a.name}
                </Tag>
              ))}
            </div>
          )}

          {/* Hidden file picker, triggered by the paperclip button */}
          <input
            ref={fileInputRef}
            type="file"
            multiple
            accept={ACCEPTED_EXTENSIONS}
            style={{ display: 'none' }}
            onChange={handleFilesPicked}
          />

          {/* Input area */}
          <Space.Compact style={{ width: '100%' }}>
            <Input.TextArea
              data-tour="chat-input"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onPressEnter={(e) => {
                if (!e.shiftKey) {
                  e.preventDefault();
                  handleSend();
                }
              }}
              placeholder="Ask a question about documents in this workspace..."
              autoSize={{ minRows: 1, maxRows: 4 }}
              disabled={loading}
              style={{ flex: 1 }}
            />
            <Tooltip
              title={
                attachments.length >= MAX_ATTACHMENTS
                  ? `Maximum ${MAX_ATTACHMENTS} attachments reached`
                  : 'Attach documents (PDF, DOCX, XLSX, CSV, HTML, Markdown, TXT) — ask about them directly'
              }
            >
              <Button
                icon={<PaperClipOutlined />}
                onClick={() => fileInputRef.current?.click()}
                disabled={loading || attachments.length >= MAX_ATTACHMENTS}
              />
            </Tooltip>
            <Tooltip title="Request timeout — increase if you have many documents or a complex pipeline">
              <Select
                value={timeoutMs}
                onChange={handleTimeoutChange}
                options={TIMEOUT_PRESETS}
                style={{ width: 130 }}
                suffixIcon={<FieldTimeOutlined />}
                disabled={loading}
              />
            </Tooltip>
            <Button
              data-tour="chat-send"
              type="primary"
              icon={<SendOutlined />}
              onClick={handleSend}
              loading={loading}
              disabled={!query.trim()}
            >
              Send
            </Button>
            <Tooltip title="Clear chat">
              <Button icon={<ClearOutlined />} onClick={handleClear} disabled={loading} />
            </Tooltip>
          </Space.Compact>
        </div>
      ) : (
        <Typography.Text type="secondary">
          Select an organization, department, and workspace to test the knowledge base.
        </Typography.Text>
      )}

      {/* Comment modal for negative feedback */}
      <Modal
        title="What went wrong?"
        open={!!commentModal}
        onOk={handleCommentSubmit}
        onCancel={() => {
          setCommentModal(null);
          setCommentText('');
        }}
        okText="Submit Feedback"
      >
        <Typography.Paragraph type="secondary">
          Your feedback helps improve the system. Optionally describe what was wrong with the answer.
        </Typography.Paragraph>
        <Input.TextArea
          value={commentText}
          onChange={(e) => setCommentText(e.target.value)}
          placeholder="e.g., Wrong information, missing context, wrong language..."
          autoSize={{ minRows: 3, maxRows: 6 }}
        />
      </Modal>
      <Tour
        open={tour.isActive}
        steps={getTestChatSteps(t)}
        onClose={tour.end}
        onFinish={tour.complete}
      />
    </>
  );
}
