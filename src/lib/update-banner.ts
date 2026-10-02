// Pure helpers for the update banner. [FRONTEND]

/** The banner shows for an offered version unless it was skipped or dismissed this session. */
export function updateBannerVisible(available: string | null, skippedVersion: string, dismissed: boolean): boolean {
  return !!available && available !== skippedVersion && !dismissed;
}

/** 0..100, or null while the total size is unknown. */
export function downloadPercent(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.floor((downloaded / total) * 100)));
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${Math.round(bytes)} B`;
}
