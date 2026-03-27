import client from './client';
import type { Job, JobListResponse } from './types';

export async function listJobs(workspaceId: string) {
  const res = await client.get<JobListResponse>(
    `/api/km/workspaces/${workspaceId}/jobs`,
  );
  return res.data;
}

export async function getJob(workspaceId: string, jobId: string) {
  const res = await client.get<Job>(
    `/api/km/workspaces/${workspaceId}/jobs/${jobId}`,
  );
  return res.data;
}

export async function cancelJob(workspaceId: string, jobId: string) {
  const res = await client.delete<{ cancelled: boolean; job_id: string }>(
    `/api/km/workspaces/${workspaceId}/jobs/${jobId}`,
  );
  return res.data;
}
