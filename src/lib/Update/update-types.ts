/** What `check_for_update` and `get_pending_update` return. Mirrors
 *  `UpdateInfo` / `UpdateCheckPayload` in `src-tauri/src/updater.rs`. */
export interface UpdateInfo {
  version: string;
  tag: string;
  current_version: string;
  notes: string;
  release_url: string;
  asset_name: string | null;
  download_size: number;
  can_self_update: boolean;
  unsupported_reason: string | null;
}

export interface UpdateCheckPayload {
  current_version: string;
  update: UpdateInfo | null;
  skipped: boolean;
}

/** Bytes downloaded so far, as emitted on `update-progress`. */
export interface UpdateProgress {
  downloaded: number;
  total: number;
}

/** A download size the user can judge at a glance. */
export function formatBytes(bytes: number): string {
  if (!bytes || bytes < 0) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/** Download progress as a percentage, or null when the server never said how
 *  large the file is — the bar shows indeterminate rather than a wrong number. */
export function progressPercent(p: UpdateProgress | null): number | null {
  if (!p || !p.total) return null;
  return Math.min(100, Math.round((p.downloaded / p.total) * 100));
}
