/**
 * The ThaiRAG wordmark. The glyph is a stacked-document mark with a celadon
 * "retrieval" line — the product reads a stack of pages and pulls the relevant
 * line out. `tone` picks legible colors for ink vs. paper backgrounds.
 */
export function BrandMark({
  tone = 'dark',
  size = 26,
}: {
  tone?: 'light' | 'dark';
  size?: number;
}) {
  const word = tone === 'light' ? 'var(--ink-bright)' : 'var(--text)';
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
      <svg width={size} height={size} viewBox="0 0 28 28" aria-hidden="true">
        <rect x="0.5" y="0.5" width="27" height="27" rx="7" fill="var(--ink)" />
        <rect x="7" y="8" width="14" height="2" rx="1" fill="var(--ink-bright)" fillOpacity="0.5" />
        <rect x="7" y="13" width="14" height="2" rx="1" fill="var(--celadon)" />
        <rect x="7" y="18" width="9" height="2" rx="1" fill="var(--ink-bright)" fillOpacity="0.5" />
      </svg>
      <span
        style={{
          fontFamily: "'IBM Plex Sans Thai','IBM Plex Sans',sans-serif",
          fontWeight: 600,
          fontSize: size * 0.66,
          letterSpacing: '-0.01em',
          color: word,
        }}
      >
        ThaiRAG
      </span>
    </div>
  );
}
