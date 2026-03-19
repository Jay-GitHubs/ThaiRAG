import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  createConnector,
  createFromTemplate,
  deleteConnector,
  listConnectors,
  listConnectorTemplates,
  listSyncRuns,
  pauseConnector,
  resumeConnector,
  testConnection,
  triggerSync,
  updateConnector,
} from '../api/connectors';
import type {
  CreateConnectorRequest,
  CreateFromTemplateRequest,
  UpdateConnectorRequest,
} from '../api/types';

export function useConnectors() {
  return useQuery({
    queryKey: ['connectors'],
    queryFn: () => listConnectors(),
  });
}

export function useConnectorTemplates() {
  return useQuery({
    queryKey: ['connector-templates'],
    queryFn: () => listConnectorTemplates(),
  });
}

export function useSyncRuns(connectorId: string | undefined) {
  return useQuery({
    queryKey: ['sync-runs', connectorId],
    queryFn: () => listSyncRuns(connectorId!),
    enabled: !!connectorId,
  });
}

export function useCreateConnector() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateConnectorRequest) => createConnector(data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['connectors'] }),
  });
}

export function useCreateFromTemplate() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateFromTemplateRequest) => createFromTemplate(data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['connectors'] }),
  });
}

export function useUpdateConnector() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateConnectorRequest }) =>
      updateConnector(id, data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['connectors'] }),
  });
}

export function useDeleteConnector() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteConnector(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['connectors'] }),
  });
}

export function useTriggerSync() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => triggerSync(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['connectors'] });
      qc.invalidateQueries({ queryKey: ['sync-runs'] });
    },
  });
}

export function usePauseConnector() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => pauseConnector(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['connectors'] }),
  });
}

export function useResumeConnector() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => resumeConnector(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['connectors'] }),
  });
}

export function useTestConnection() {
  return useMutation({
    mutationFn: (id: string) => testConnection(id),
  });
}
