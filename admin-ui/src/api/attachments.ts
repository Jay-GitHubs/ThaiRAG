/**
 * Per-request document attachments for the Test Chat page.
 *
 * Files are base64-encoded client-side and sent inline with the test-query
 * request. The backend extracts text, runs input guardrails, and answers
 * from the attachment documents (skipping workspace search).
 */

/** Wire format for a single attachment — mirrors the backend `Attachment`. */
export interface Attachment {
  name: string;
  /** MIME type from the document pipeline's supported list. */
  mime_type: string;
  /** Base64-encoded raw file bytes (no `data:` prefix). */
  data: string;
}

/** Extension → MIME map for the formats the backend converter accepts. */
const EXT_MIME: Record<string, string> = {
  pdf: 'application/pdf',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  csv: 'text/csv',
  html: 'text/html',
  htm: 'text/html',
  md: 'text/markdown',
  markdown: 'text/markdown',
  txt: 'text/plain',
};

/** `accept` attribute value for the file picker. */
export const ACCEPTED_EXTENSIONS = '.pdf,.docx,.xlsx,.csv,.html,.htm,.md,.markdown,.txt';

/** Default 5 MB per-file ceiling — matches the backend's `max_bytes_per_attachment`. */
export const MAX_ATTACHMENT_BYTES = 5 * 1024 * 1024;

/** Max attachments per request — matches the backend's `max_per_request`. */
export const MAX_ATTACHMENTS = 5;

/**
 * Resolve a file's MIME type. The extension map wins when the extension is
 * known (browsers report an empty `type` for `.md` and friends); otherwise
 * fall back to the browser-supplied type.
 */
function resolveMime(file: File): string {
  const ext = file.name.split('.').pop()?.toLowerCase() ?? '';
  return EXT_MIME[ext] ?? file.type ?? 'application/octet-stream';
}

/** Read a `File` and produce an `Attachment` with base64-encoded contents. */
export function fileToAttachment(file: File): Promise<Attachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      // FileReader.readAsDataURL yields "data:<mime>;base64,<payload>".
      const comma = result.indexOf(',');
      resolve({
        name: file.name,
        mime_type: resolveMime(file),
        data: comma >= 0 ? result.slice(comma + 1) : result,
      });
    };
    reader.onerror = () => reject(reader.error ?? new Error(`Failed to read ${file.name}`));
    reader.readAsDataURL(file);
  });
}
