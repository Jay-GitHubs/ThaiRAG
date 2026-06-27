import { useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeRaw from 'rehype-raw';
import rehypeSanitize from 'rehype-sanitize';

// Keep alphanumerics only (Latin + Thai + digits) so a converted-markdown row
// matches regardless of pipes/whitespace/punctuation.
const norm = (s: string) => s.replace(/[^0-9a-z฀-๿]/gi, '').toLowerCase();

function textOf(node: ReactNode): string {
  if (node == null || node === false) return '';
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(textOf).join('');
  const el = node as { props?: { children?: ReactNode } };
  if (el.props) return textOf(el.props.children);
  return '';
}

/**
 * Renders the converted document text as markdown — so tables (xlsx/docx/csv),
 * headings and lists render faithfully — and highlights the block (table row,
 * paragraph, or list item) that contains the cited passage, for fast
 * verification. Block-level highlight works across formats and rendered markup.
 */
export function RichTextView({ content, snippet }: { content: string; snippet?: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sn = snippet ? norm(snippet) : '';
  const snPrefix = sn.slice(0, 40);

  const isHit = (children: ReactNode): boolean => {
    if (!sn) return false;
    const t = norm(textOf(children));
    if (t.length < 6) return false;
    // Short blocks (table rows): the row text is part of the snippet.
    // Long blocks (paragraphs): the block contains the snippet's start.
    return sn.includes(t) || (snPrefix.length >= 10 && t.includes(snPrefix));
  };
  const hl = (children: ReactNode) =>
    isHit(children)
      ? {
          'data-testid': 'source-highlight',
          style: { background: 'var(--mark-bg)', color: 'var(--mark-text)' },
        }
      : {};

  // Scroll the first highlighted block into view once rendered.
  useEffect(() => {
    const el = containerRef.current?.querySelector('[data-testid="source-highlight"]');
    el?.scrollIntoView({ block: 'center' });
  }, [content, snippet]);

  return (
    <div ref={containerRef} data-testid="source-content" className="md-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        // Document converters emit raw HTML (e.g. docx → <table> with merged
        // cells); rehype-raw renders it, rehype-sanitize strips anything unsafe.
        rehypePlugins={[rehypeRaw, rehypeSanitize]}
        components={{
          tr: ({ children }) => <tr {...hl(children)}>{children}</tr>,
          p: ({ children }) => <p {...hl(children)}>{children}</p>,
          li: ({ children }) => <li {...hl(children)}>{children}</li>,
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
