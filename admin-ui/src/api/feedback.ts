import client from './client';
import type { FeedbackRequest, FeedbackResponse } from './types';

export async function submitFeedback(data: FeedbackRequest) {
  const res = await client.post<FeedbackResponse>('/v1/chat/feedback', data);
  return res.data;
}
