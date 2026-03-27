import { useState, useEffect, useCallback, useRef } from 'react';
import {
  Card,
  Table,
  Button,
  Space,
  Typography,
  Select,
  Input,
  Tag,
  Drawer,
  Descriptions,
  List,
  Popconfirm,
  message,
  Spin,
  Empty,
  Row,
  Col,
  Statistic,
  Tooltip,
} from 'antd';
import {
  DeleteOutlined,
  SearchOutlined,
  ReloadOutlined,
  NodeIndexOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import {
  listEntities,
  getEntity,
  getKnowledgeGraph,
  deleteEntity,
  extractFromDocument,
} from '../api/knowledgeGraph';
import type {
  Entity,
  Relation,
  EntityWithRelations,
  KnowledgeGraph,
} from '../api/knowledgeGraph';

const { Title, Text } = Typography;

// ── Entity type colors ──────────────────────────────────────────────

const ENTITY_TYPE_COLORS: Record<string, string> = {
  Person: 'blue',
  Organization: 'green',
  Location: 'orange',
  Concept: 'purple',
  Event: 'red',
  Technology: 'cyan',
  Product: 'magenta',
};

const ENTITY_TYPES = [
  'Person',
  'Organization',
  'Location',
  'Concept',
  'Event',
  'Technology',
  'Product',
];

// ── Workspace selector (reuse pattern from DocumentsPage) ───────────

interface WorkspaceOption {
  id: string;
  name: string;
  dept_name?: string;
}

// ── Force-directed graph visualization ──────────────────────────────

interface GraphNode {
  id: string;
  label: string;
  type: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
}

interface GraphEdge {
  from: string;
  to: string;
  label: string;
  confidence: number;
}

function ForceGraph({
  entities,
  relations,
  onNodeClick,
}: {
  entities: Entity[];
  relations: Relation[];
  onNodeClick: (entityId: string) => void;
}) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [nodes, setNodes] = useState<GraphNode[]>([]);
  const [edges, setEdges] = useState<GraphEdge[]>([]);
  const [dimensions, setDimensions] = useState({ width: 800, height: 500 });
  const animRef = useRef<number>();

  useEffect(() => {
    if (!svgRef.current) return;
    const rect = svgRef.current.parentElement?.getBoundingClientRect();
    if (rect) {
      setDimensions({ width: rect.width, height: Math.max(400, rect.height) });
    }
  }, [entities]);

  useEffect(() => {
    if (entities.length === 0) return;
    const w = dimensions.width;
    const h = dimensions.height;

    // Initialize nodes at random positions
    const initialNodes: GraphNode[] = entities.map((e) => ({
      id: e.id,
      label: e.name.length > 20 ? e.name.slice(0, 18) + '...' : e.name,
      type: e.entity_type,
      x: w * 0.2 + Math.random() * w * 0.6,
      y: h * 0.2 + Math.random() * h * 0.6,
      vx: 0,
      vy: 0,
    }));

    const graphEdges: GraphEdge[] = relations.map((r) => ({
      from: r.from_entity_id,
      to: r.to_entity_id,
      label: r.relation_type,
      confidence: r.confidence,
    }));

    setEdges(graphEdges);

    // Simple force simulation (runs for 100 iterations)
    let iteration = 0;
    const maxIter = 80;

    const simulate = () => {
      if (iteration >= maxIter) return;
      iteration++;

      // Repulsion between all nodes
      for (let i = 0; i < initialNodes.length; i++) {
        for (let j = i + 1; j < initialNodes.length; j++) {
          const dx = initialNodes[j].x - initialNodes[i].x;
          const dy = initialNodes[j].y - initialNodes[i].y;
          const dist = Math.max(Math.sqrt(dx * dx + dy * dy), 1);
          const force = 5000 / (dist * dist);
          const fx = (dx / dist) * force;
          const fy = (dy / dist) * force;
          initialNodes[i].vx -= fx;
          initialNodes[i].vy -= fy;
          initialNodes[j].vx += fx;
          initialNodes[j].vy += fy;
        }
      }

      // Attraction along edges
      const nodeMap = new Map(initialNodes.map((n) => [n.id, n]));
      for (const edge of graphEdges) {
        const a = nodeMap.get(edge.from);
        const b = nodeMap.get(edge.to);
        if (!a || !b) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dist = Math.max(Math.sqrt(dx * dx + dy * dy), 1);
        const force = dist * 0.01;
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        a.vx += fx;
        a.vy += fy;
        b.vx -= fx;
        b.vy -= fy;
      }

      // Center gravity
      for (const node of initialNodes) {
        node.vx += (w / 2 - node.x) * 0.001;
        node.vy += (h / 2 - node.y) * 0.001;
      }

      // Apply velocities with damping
      const damping = 0.8;
      for (const node of initialNodes) {
        node.vx *= damping;
        node.vy *= damping;
        node.x += node.vx;
        node.y += node.vy;
        // Keep within bounds
        node.x = Math.max(60, Math.min(w - 60, node.x));
        node.y = Math.max(30, Math.min(h - 30, node.y));
      }

      setNodes([...initialNodes]);
      animRef.current = requestAnimationFrame(simulate);
    };

    simulate();

    return () => {
      if (animRef.current) cancelAnimationFrame(animRef.current);
    };
  }, [entities, relations, dimensions]);

  if (entities.length === 0) {
    return <Empty description="No entities to visualize" />;
  }

  const nodeMap = new Map(nodes.map((n) => [n.id, n]));

  return (
    <svg
      ref={svgRef}
      width="100%"
      height={dimensions.height}
      style={{ border: '1px solid #d9d9d9', borderRadius: 6, background: '#fafafa' }}
    >
      <defs>
        <marker id="arrowhead" markerWidth="6" markerHeight="4" refX="6" refY="2" orient="auto">
          <polygon points="0 0, 6 2, 0 4" fill="#999" />
        </marker>
      </defs>

      {/* Edges */}
      {edges.map((edge, i) => {
        const from = nodeMap.get(edge.from);
        const to = nodeMap.get(edge.to);
        if (!from || !to) return null;
        return (
          <g key={`edge-${i}`}>
            <line
              x1={from.x}
              y1={from.y}
              x2={to.x}
              y2={to.y}
              stroke="#999"
              strokeWidth={Math.max(1, edge.confidence * 2)}
              strokeOpacity={0.6}
              markerEnd="url(#arrowhead)"
            />
            <text
              x={(from.x + to.x) / 2}
              y={(from.y + to.y) / 2 - 5}
              fontSize={9}
              fill="#666"
              textAnchor="middle"
            >
              {edge.label}
            </text>
          </g>
        );
      })}

      {/* Nodes */}
      {nodes.map((node) => {
        const color = ENTITY_TYPE_COLORS[node.type] || '#999';
        return (
          <g
            key={node.id}
            style={{ cursor: 'pointer' }}
            onClick={() => onNodeClick(node.id)}
          >
            <circle cx={node.x} cy={node.y} r={18} fill={color} opacity={0.8} />
            <text
              x={node.x}
              y={node.y + 30}
              fontSize={10}
              textAnchor="middle"
              fill="#333"
            >
              {node.label}
            </text>
            <text
              x={node.x}
              y={node.y + 4}
              fontSize={10}
              textAnchor="middle"
              fill="white"
              fontWeight="bold"
            >
              {node.type.charAt(0)}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

// ── Main Page Component ─────────────────────────────────────────────

export default function KnowledgeGraphPage() {
  const [workspaces, setWorkspaces] = useState<WorkspaceOption[]>([]);
  const [selectedWorkspace, setSelectedWorkspace] = useState<string>('');
  const [entities, setEntities] = useState<Entity[]>([]);
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);
  const [searchQuery, setSearchQuery] = useState('');
  const [loading, setLoading] = useState(false);
  const [drawerVisible, setDrawerVisible] = useState(false);
  const [selectedEntity, setSelectedEntity] = useState<EntityWithRelations | null>(null);
  const [graphData, setGraphData] = useState<KnowledgeGraph | null>(null);
  const [viewMode, setViewMode] = useState<'table' | 'graph'>('table');

  // Load workspaces
  useEffect(() => {
    (async () => {
      try {
        const { default: client } = await import('../api/client');
        const res = await client.get('/api/km/workspaces/all');
        const data = res.data?.items || res.data || [];
        setWorkspaces(data);
        if (data.length > 0 && !selectedWorkspace) {
          setSelectedWorkspace(data[0].id);
        }
      } catch {
        // Fallback: try listing from orgs
        try {
          const { default: client } = await import('../api/client');
          const orgsRes = await client.get('/api/km/orgs');
          const orgs = orgsRes.data?.items || orgsRes.data || [];
          const allWs: WorkspaceOption[] = [];
          for (const org of orgs) {
            const deptsRes = await client.get(`/api/km/orgs/${org.id}/depts`);
            const depts = deptsRes.data?.items || deptsRes.data || [];
            for (const dept of depts) {
              const wsRes = await client.get(
                `/api/km/orgs/${org.id}/depts/${dept.id}/workspaces`,
              );
              const wsList = wsRes.data?.items || wsRes.data || [];
              for (const ws of wsList) {
                allWs.push({ id: ws.id, name: ws.name, dept_name: dept.name });
              }
            }
          }
          setWorkspaces(allWs);
          if (allWs.length > 0 && !selectedWorkspace) {
            setSelectedWorkspace(allWs[0].id);
          }
        } catch (e2) {
          console.error('Failed to load workspaces:', e2);
        }
      }
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Load entities when workspace/filter changes
  const loadEntities = useCallback(async () => {
    if (!selectedWorkspace) return;
    setLoading(true);
    try {
      const params: Record<string, string> = {};
      if (typeFilter) params.type = typeFilter;
      if (searchQuery) params.q = searchQuery;
      const data = await listEntities(selectedWorkspace, params);
      setEntities(data);
    } catch (err) {
      message.error('Failed to load entities');
      console.error(err);
    } finally {
      setLoading(false);
    }
  }, [selectedWorkspace, typeFilter, searchQuery]);

  useEffect(() => {
    loadEntities();
  }, [loadEntities]);

  // Load graph data
  const loadGraph = useCallback(async () => {
    if (!selectedWorkspace) return;
    setLoading(true);
    try {
      const data = await getKnowledgeGraph(selectedWorkspace);
      setGraphData(data);
    } catch (err) {
      message.error('Failed to load knowledge graph');
      console.error(err);
    } finally {
      setLoading(false);
    }
  }, [selectedWorkspace]);

  useEffect(() => {
    if (viewMode === 'graph') {
      loadGraph();
    }
  }, [viewMode, loadGraph]);

  const handleEntityClick = async (entityId: string) => {
    if (!selectedWorkspace) return;
    try {
      const data = await getEntity(selectedWorkspace, entityId);
      setSelectedEntity(data);
      setDrawerVisible(true);
    } catch (err) {
      message.error('Failed to load entity details');
    }
  };

  const handleDeleteEntity = async (entityId: string) => {
    if (!selectedWorkspace) return;
    try {
      await deleteEntity(selectedWorkspace, entityId);
      message.success('Entity deleted');
      loadEntities();
      if (viewMode === 'graph') loadGraph();
    } catch (err) {
      message.error('Failed to delete entity');
    }
  };

  const handleExtract = async (docId: string) => {
    if (!selectedWorkspace) return;
    try {
      const result = await extractFromDocument(selectedWorkspace, docId);
      message.success(
        `Extracted ${result.entities_created} entities, ${result.relations_created} relations`,
      );
      loadEntities();
      if (viewMode === 'graph') loadGraph();
    } catch (err: any) {
      const msg = err?.response?.data?.error?.message || 'Extraction failed';
      message.error(msg);
    }
  };

  const columns: ColumnsType<Entity> = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: Entity) => (
        <Button type="link" onClick={() => handleEntityClick(record.id)}>
          {name}
        </Button>
      ),
    },
    {
      title: 'Type',
      dataIndex: 'entity_type',
      key: 'entity_type',
      width: 130,
      render: (type: string) => (
        <Tag color={ENTITY_TYPE_COLORS[type] || 'default'}>{type}</Tag>
      ),
    },
    {
      title: 'Documents',
      dataIndex: 'doc_ids',
      key: 'doc_ids',
      width: 100,
      render: (docIds: string[]) => docIds.length,
    },
    {
      title: 'Created',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 180,
      render: (v: string) => {
        try {
          return new Date(v).toLocaleString();
        } catch {
          return v;
        }
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 80,
      render: (_: unknown, record: Entity) => (
        <Popconfirm
          title="Delete this entity and all its relations?"
          onConfirm={() => handleDeleteEntity(record.id)}
        >
          <Button danger icon={<DeleteOutlined />} size="small" />
        </Popconfirm>
      ),
    },
  ];

  return (
    <div>
      <Title level={3}>Knowledge Graph</Title>
      <Text type="secondary">
        Explore entities and relationships extracted from your documents.
      </Text>

      <Card style={{ marginTop: 16 }}>
        <Space wrap style={{ marginBottom: 16 }}>
          <Select
            placeholder="Select workspace"
            value={selectedWorkspace || undefined}
            onChange={setSelectedWorkspace}
            style={{ width: 250 }}
            options={workspaces.map((ws) => ({
              value: ws.id,
              label: ws.dept_name ? `${ws.dept_name} / ${ws.name}` : ws.name,
            }))}
          />

          <Select
            placeholder="Filter by type"
            value={typeFilter}
            onChange={setTypeFilter}
            allowClear
            style={{ width: 160 }}
            options={ENTITY_TYPES.map((t) => ({ value: t, label: t }))}
          />

          <Input
            placeholder="Search entities..."
            prefix={<SearchOutlined />}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            style={{ width: 200 }}
            allowClear
          />

          <Button.Group>
            <Button
              type={viewMode === 'table' ? 'primary' : 'default'}
              onClick={() => setViewMode('table')}
            >
              Table
            </Button>
            <Button
              type={viewMode === 'graph' ? 'primary' : 'default'}
              onClick={() => setViewMode('graph')}
              icon={<NodeIndexOutlined />}
            >
              Graph
            </Button>
          </Button.Group>

          <Button icon={<ReloadOutlined />} onClick={viewMode === 'graph' ? loadGraph : loadEntities}>
            Refresh
          </Button>
        </Space>

        {/* Stats row */}
        {(viewMode === 'graph' && graphData) && (
          <Row gutter={16} style={{ marginBottom: 16 }}>
            <Col span={6}>
              <Statistic title="Entities" value={graphData.entities.length} />
            </Col>
            <Col span={6}>
              <Statistic title="Relations" value={graphData.relations.length} />
            </Col>
            <Col span={6}>
              <Statistic
                title="Entity Types"
                value={new Set(graphData.entities.map((e) => e.entity_type)).size}
              />
            </Col>
            <Col span={6}>
              <Statistic
                title="Relation Types"
                value={new Set(graphData.relations.map((r) => r.relation_type)).size}
              />
            </Col>
          </Row>
        )}

        <Spin spinning={loading}>
          {viewMode === 'table' ? (
            <Table
              dataSource={entities}
              columns={columns}
              rowKey="id"
              pagination={{ pageSize: 20, showSizeChanger: true }}
              size="middle"
            />
          ) : graphData ? (
            <ForceGraph
              entities={graphData.entities}
              relations={graphData.relations}
              onNodeClick={handleEntityClick}
            />
          ) : (
            <Empty description="Select a workspace to view the knowledge graph" />
          )}
        </Spin>
      </Card>

      {/* Entity Detail Drawer */}
      <Drawer
        title={selectedEntity?.name || 'Entity Details'}
        open={drawerVisible}
        onClose={() => setDrawerVisible(false)}
        width={500}
      >
        {selectedEntity && (
          <div>
            <Descriptions column={1} bordered size="small">
              <Descriptions.Item label="Type">
                <Tag color={ENTITY_TYPE_COLORS[selectedEntity.entity_type] || 'default'}>
                  {selectedEntity.entity_type}
                </Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Documents">
                {selectedEntity.doc_ids.length}
              </Descriptions.Item>
              <Descriptions.Item label="Created">
                {new Date(selectedEntity.created_at).toLocaleString()}
              </Descriptions.Item>
              <Descriptions.Item label="ID">
                <Text copyable code style={{ fontSize: 11 }}>
                  {selectedEntity.id}
                </Text>
              </Descriptions.Item>
            </Descriptions>

            <Title level={5} style={{ marginTop: 24 }}>
              Relations ({selectedEntity.relations?.length || 0})
            </Title>

            {selectedEntity.relations && selectedEntity.relations.length > 0 ? (
              <List
                size="small"
                dataSource={selectedEntity.relations}
                renderItem={(rel: Relation) => {
                  const isFrom = rel.from_entity_id === selectedEntity.id;
                  const otherEntityId = isFrom ? rel.to_entity_id : rel.from_entity_id;
                  const direction = isFrom ? '\u2192' : '\u2190';
                  return (
                    <List.Item>
                      <Space>
                        <Text>{direction}</Text>
                        <Tag>{rel.relation_type}</Tag>
                        <Tooltip title={`Confidence: ${(rel.confidence * 100).toFixed(0)}%`}>
                          <Button
                            type="link"
                            size="small"
                            onClick={() => handleEntityClick(otherEntityId)}
                          >
                            {otherEntityId.slice(0, 8)}...
                          </Button>
                        </Tooltip>
                      </Space>
                    </List.Item>
                  );
                }}
              />
            ) : (
              <Empty description="No relations" image={Empty.PRESENTED_IMAGE_SIMPLE} />
            )}

            <Title level={5} style={{ marginTop: 24 }}>
              Document Links
            </Title>
            {selectedEntity.doc_ids.length > 0 ? (
              <List
                size="small"
                dataSource={selectedEntity.doc_ids}
                renderItem={(docId: string) => (
                  <List.Item
                    actions={[
                      <Tooltip title="Extract entities from this document" key="extract">
                        <Button
                          icon={<ThunderboltOutlined />}
                          size="small"
                          onClick={() => handleExtract(docId)}
                        >
                          Re-extract
                        </Button>
                      </Tooltip>,
                    ]}
                  >
                    <Text code style={{ fontSize: 11 }}>
                      {docId.slice(0, 8)}...
                    </Text>
                  </List.Item>
                )}
              />
            ) : (
              <Empty description="No linked documents" image={Empty.PRESENTED_IMAGE_SIMPLE} />
            )}
          </div>
        )}
      </Drawer>
    </div>
  );
}
