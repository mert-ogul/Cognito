/** Basename of a path, accepting `/` or `\` separators. */
export function fileName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

/** Page range label: `p. 3`, `pp. 3-5`, or null when start is missing. */
export function pageLabel(
  pageStart: number | null | undefined,
  pageEnd: number | null | undefined,
): string | null {
  if (pageStart != null && pageEnd != null && pageEnd !== pageStart) {
    return `pp. ${pageStart}-${pageEnd}`;
  }
  if (pageStart != null) {
    return `p. ${pageStart}`;
  }
  return null;
}

/** Human size for model download progress. */
export function formatModelBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 MB";
  const gb = n / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  const mb = n / (1024 * 1024);
  return `${Math.max(1, Math.round(mb))} MB`;
}
