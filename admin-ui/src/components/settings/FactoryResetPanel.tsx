import { useEffect, useState } from 'react';
import { Alert, Button, Input, Popconfirm, Radio, Select, Space, Typography, message } from 'antd';
import { factoryReset } from '../../api/settings';
import { listDepts, listOrgs, listWorkspaces } from '../../api/km';

type Level = 'global' | 'org' | 'dept' | 'workspace';
type Opt = { label: string; value: string };

// KM list endpoints return { data, total }; tolerate a bare array too.
function toOptions(res: unknown): Opt[] {
  const arr = (res as { data?: unknown[] })?.data ?? (res as unknown[]);
  if (!Array.isArray(arr)) return [];
  return arr.map((x) => {
    const o = x as { id: string; name?: string };
    return { label: o.name ?? o.id, value: o.id };
  });
}

/** Destructive factory-reset control: pick a scope (global or a specific
 *  org/dept/workspace), choose content-vs-full for global, type RESET, confirm. */
export function FactoryResetPanel({ onDone }: { onDone?: () => void }) {
  const [level, setLevel] = useState<Level>('global');
  const [mode, setMode] = useState<'content' | 'full'>('content');
  const [confirm, setConfirm] = useState('');
  const [busy, setBusy] = useState(false);

  const [orgs, setOrgs] = useState<Opt[]>([]);
  const [depts, setDepts] = useState<Opt[]>([]);
  const [workspaces, setWorkspaces] = useState<Opt[]>([]);
  const [orgId, setOrgId] = useState<string>();
  const [deptId, setDeptId] = useState<string>();
  const [wsId, setWsId] = useState<string>();

  useEffect(() => {
    if (level === 'global') return;
    listOrgs().then((r) => setOrgs(toOptions(r))).catch(() => setOrgs([]));
  }, [level]);

  useEffect(() => {
    setDepts([]);
    setDeptId(undefined);
    setWorkspaces([]);
    setWsId(undefined);
    if (!orgId || level === 'global' || level === 'org') return;
    listDepts(orgId).then((r) => setDepts(toOptions(r))).catch(() => setDepts([]));
  }, [orgId, level]);

  useEffect(() => {
    setWorkspaces([]);
    setWsId(undefined);
    if (!orgId || !deptId || level !== 'workspace') return;
    listWorkspaces(orgId, deptId).then((r) => setWorkspaces(toOptions(r))).catch(() => setWorkspaces([]));
  }, [orgId, deptId, level]);

  const targetId = level === 'org' ? orgId : level === 'dept' ? deptId : level === 'workspace' ? wsId : undefined;
  const scopeReady = level === 'global' || !!targetId;
  const canReset = confirm.trim() === 'RESET' && scopeReady && !busy;

  const handleReset = async () => {
    setBusy(true);
    try {
      const scope =
        level === 'global' ? ({ level: 'global' } as const) : ({ level, id: targetId! } as const);
      const res = await factoryReset({
        scope,
        mode: level === 'global' ? mode : 'content',
        confirm: 'RESET',
      });
      message.success(`Factory reset complete — ${res.summary}`);
      setConfirm('');
      onDone?.();
    } catch {
      message.error('Factory reset failed');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-testid="factory-reset">
      <Alert
        type="error"
        showIcon
        style={{ marginBottom: 16 }}
        message="Factory reset permanently deletes data"
        description="A global reset wipes all documents, chunks, vectors, the BM25 index, knowledge graph and conversations. 'Full' additionally removes users, organizations, workspaces and settings (back to first-run). A scoped reset wipes only the chosen Org / Dept / Workspace's content. This cannot be undone."
      />
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <div>
          <Typography.Text strong>Scope</Typography.Text>
          <br />
          <Select
            data-testid="reset-scope"
            value={level}
            onChange={(v) => setLevel(v as Level)}
            style={{ width: 260 }}
            options={[
              { value: 'global', label: 'Everything (global)' },
              { value: 'org', label: 'Organization' },
              { value: 'dept', label: 'Department' },
              { value: 'workspace', label: 'Workspace' },
            ]}
          />
        </div>

        {level !== 'global' && (
          <Select
            placeholder="Select organization"
            style={{ width: 340 }}
            value={orgId}
            onChange={setOrgId}
            options={orgs}
          />
        )}
        {(level === 'dept' || level === 'workspace') && (
          <Select
            placeholder="Select department"
            style={{ width: 340 }}
            value={deptId}
            onChange={setDeptId}
            options={depts}
            disabled={!orgId}
          />
        )}
        {level === 'workspace' && (
          <Select
            placeholder="Select workspace"
            style={{ width: 340 }}
            value={wsId}
            onChange={setWsId}
            options={workspaces}
            disabled={!deptId}
          />
        )}

        {level === 'global' && (
          <div>
            <Typography.Text strong>Mode</Typography.Text>
            <br />
            <Radio.Group value={mode} onChange={(e) => setMode(e.target.value)} data-testid="reset-mode">
              <Radio value="content">Content only (keep users &amp; structure)</Radio>
              <Radio value="full">Full (also wipe users, orgs, settings)</Radio>
            </Radio.Group>
          </div>
        )}

        <div>
          <Typography.Text strong>Type RESET to confirm</Typography.Text>
          <br />
          <Input
            data-testid="reset-confirm"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            placeholder="RESET"
            style={{ width: 260 }}
          />
        </div>

        <Popconfirm
          title="Run factory reset?"
          description="This permanently deletes data and cannot be undone."
          okText="Yes, reset"
          okButtonProps={{ danger: true }}
          onConfirm={handleReset}
          disabled={!canReset}
        >
          <Button danger data-testid="reset-submit" disabled={!canReset} loading={busy}>
            Factory Reset
          </Button>
        </Popconfirm>
      </Space>
    </div>
  );
}
