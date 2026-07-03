import { listMessages } from '../api/conversations';
import { parseCitations } from '../api/types';
import type { Conversation } from '../api/types';

/** Fetch a conversation's turns and download them as a Markdown file:
 *  title header, one section per turn, and each answer's sources as a
 *  numbered footnote list. Client-side only — no backend endpoint. */
export async function exportConversationMarkdown(conversation: Conversation): Promise<void> {
  const rows = await listMessages(conversation.id);
  const title = conversation.title?.trim() || 'conversation';

  const lines: string[] = [`# ${title}`, ''];
  for (const r of rows) {
    lines.push(r.role === 'user' ? '## 🧑 User' : '## 🤖 Assistant', '', r.content.trim(), '');
    const citations = parseCitations(r.citations);
    if (citations.length > 0) {
      lines.push('**Sources:**', '');
      citations.forEach((c, i) => {
        const loc = [c.page ? `p.${c.page}` : null, c.section || null].filter(Boolean).join(', ');
        lines.push(`${i + 1}. ${c.title}${loc ? ` (${loc})` : ''}`);
      });
      lines.push('');
    }
  }

  const blob = new Blob([lines.join('\n')], { type: 'text/markdown;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  // Keep Thai characters; strip only filesystem-hostile ones.
  a.download = `${title.replace(/[\\/:*?"<>|]/g, '_').slice(0, 80)}.md`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
