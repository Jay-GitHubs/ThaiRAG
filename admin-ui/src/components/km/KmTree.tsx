import { useEffect, useState, useCallback } from 'react';
import { Tree, Button, Modal, Input, message, Space } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import type { DataNode, EventDataNode } from 'antd/es/tree';
import { listOrgs, listDepts, listWorkspaces } from '../../api/km';
import { useCreateOrg } from '../../hooks/useOrgs';
import type { KmSelection } from '../../pages/KmPage';

interface Props {
  onSelect: (sel: KmSelection | null) => void;
  refreshKey: number;
  onMutated: () => void;
}

export function KmTree({ onSelect, refreshKey, onMutated }: Props) {
  const [treeData, setTreeData] = useState<DataNode[]>([]);
  const [expandedKeys, setExpandedKeys] = useState<React.Key[]>([]);
  const [createOpen, setCreateOpen] = useState(false);
  const [newName, setNewName] = useState('');
  const createOrg = useCreateOrg();

  const reloadTree = useCallback(async () => {
    try {
      const res = await listOrgs();
      let newTree: DataNode[] = res.data.map((org) => ({
        title: org.name,
        key: `org:${org.id}`,
        isLeaf: false,
      }));

      // Reload children for all currently expanded nodes
      for (const key of expandedKeys) {
        const parts = (key as string).split(':');
        if (parts[0] === 'org') {
          const orgId = parts[1];
          try {
            const deptRes = await listDepts(orgId);
            const children = deptRes.data.map((d) => ({
              title: d.name,
              key: `dept:${orgId}:${d.id}`,
              isLeaf: false,
            }));
            newTree = updateTreeChildren(newTree, key as string, children);
          } catch {
            // If org was deleted, skip
          }
        } else if (parts[0] === 'dept') {
          const orgId = parts[1];
          const deptId = parts[2];
          try {
            const wsRes = await listWorkspaces(orgId, deptId);
            const children = wsRes.data.map((w) => ({
              title: w.name,
              key: `ws:${orgId}:${deptId}:${w.id}`,
              isLeaf: true,
            }));
            newTree = updateTreeChildren(newTree, key as string, children);
          } catch {
            // If dept was deleted, skip
          }
        }
      }

      setTreeData(newTree);
    } catch {
      // silently fail if not authenticated yet
    }
  }, [expandedKeys]);

  useEffect(() => {
    reloadTree();
  }, [refreshKey, reloadTree]);

  async function onLoadData(node: EventDataNode<DataNode>) {
    const key = node.key as string;
    const parts = key.split(':');

    if (parts[0] === 'org') {
      const orgId = parts[1];
      const res = await listDepts(orgId);
      const children = res.data.map((d) => ({
        title: d.name,
        key: `dept:${orgId}:${d.id}`,
        isLeaf: false,
      }));
      setTreeData((prev) => updateTreeChildren(prev, key, children));
    } else if (parts[0] === 'dept') {
      const orgId = parts[1];
      const deptId = parts[2];
      const res = await listWorkspaces(orgId, deptId);
      const children = res.data.map((w) => ({
        title: w.name,
        key: `ws:${orgId}:${deptId}:${w.id}`,
        isLeaf: true,
      }));
      setTreeData((prev) => updateTreeChildren(prev, key, children));
    }
  }

  function handleExpand(keys: React.Key[]) {
    setExpandedKeys(keys);
  }

  function handleSelect(keys: React.Key[]) {
    if (keys.length === 0) {
      onSelect(null);
      return;
    }
    const key = keys[0] as string;
    const parts = key.split(':');
    if (parts[0] === 'org') {
      onSelect({ type: 'org', orgId: parts[1] });
    } else if (parts[0] === 'dept') {
      onSelect({ type: 'dept', orgId: parts[1], deptId: parts[2] });
    } else if (parts[0] === 'ws') {
      onSelect({ type: 'workspace', orgId: parts[1], deptId: parts[2], wsId: parts[3] });
    }
  }

  async function handleCreateOrg() {
    if (!newName.trim()) return;
    try {
      await createOrg.mutateAsync(newName.trim());
      setCreateOpen(false);
      setNewName('');
      onMutated();
    } catch {
      message.error('Failed to create organization');
    }
  }

  return (
    <div>
      <Space style={{ marginBottom: 8 }}>
        <Button icon={<PlusOutlined />} size="small" onClick={() => setCreateOpen(true)}>
          New Org
        </Button>
      </Space>
      <Tree
        showLine
        loadData={onLoadData}
        treeData={treeData}
        expandedKeys={expandedKeys}
        onExpand={handleExpand}
        onSelect={handleSelect}
      />
      <Modal
        title="Create Organization"
        open={createOpen}
        onOk={handleCreateOrg}
        onCancel={() => setCreateOpen(false)}
        confirmLoading={createOrg.isPending}
      >
        <Input
          placeholder="Organization name"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onPressEnter={handleCreateOrg}
        />
      </Modal>
    </div>
  );
}

function updateTreeChildren(
  tree: DataNode[],
  parentKey: string,
  children: DataNode[],
): DataNode[] {
  return tree.map((node) => {
    if (node.key === parentKey) {
      return { ...node, children };
    }
    if (node.children) {
      return { ...node, children: updateTreeChildren(node.children, parentKey, children) };
    }
    return node;
  });
}
