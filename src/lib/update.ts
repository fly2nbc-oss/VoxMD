/**
 * The update check, frontend side. The backend (`update.rs`) only asks GitHub
 * for the latest release number; *whether and when* to ask lives here: off until
 * the user says yes, at most once a day, a hint once per newer version.
 * Nothing is downloaded or installed — a newer version is named with a link.
 */

export const REPOSITORY_URL = "https://github.com/fly2nbc-oss/VoxMD";
export const UPDATE_KEY = "updateState";

/** One day between two automatic checks. */
export const DAY_MS = 24 * 60 * 60 * 1000;
/** The automatic check waits this long after the start, so the start never waits for it. */
export const CHECK_DELAY_MS = 10_000;
/** The first-start question waits until the window has stood still this long. */
export const ASK_DELAY_MS = 2_000;

export interface UpdateState {
  /** The switch; off unless the user turned it on (the question or About). */
  enabled: boolean;
  /** The user has decided — answered the question or used the switch. */
  asked: boolean;
  /** When the last check got an answer. */
  checkedMs: number | null;
  /** The latest version that check found. */
  latest: string | null;
  /** The newer version the hint was shown for. */
  told: string | null;
}

export type UpdateStatus = "never" | "checking" | "current" | "newer" | "failed";

export const defaultUpdateState = (): UpdateState => ({
  enabled: false,
  asked: false,
  checkedMs: null,
  latest: null,
  told: null,
});

/** `1.10.0` or `v1.10.0`; anything else — a pre-release suffix, a fourth number — is none. */
export function parseVersion(text: string): [number, number, number] | null {
  const parts = text.trim().replace(/^v/, "").split(".");
  if (parts.length !== 3 || !parts.every((p) => /^\d+$/.test(p))) return null;
  return [Number(parts[0]), Number(parts[1]), Number(parts[2])];
}

/** Whether `latest` is a newer release than `current`; unreadable versions are never newer. */
export function isNewer(latest: string, current: string): boolean {
  const a = parseVersion(latest);
  const b = parseVersion(current);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] > b[i];
  }
  return false;
}

/** Whether the automatic check is due: switched on, and none for a day — a clock set
 *  back counts as due, or it would wait until it caught up. */
export function due(enabled: boolean, lastMs: number | null, nowMs: number): boolean {
  return enabled && (lastMs === null || nowMs < lastMs || nowMs - lastMs >= DAY_MS);
}

export function releaseUrl(version: string): string {
  return `${REPOSITORY_URL}/releases/tag/v${version.replace(/^v/, "")}`;
}

/** What About says before anything was checked this session. */
export function statusFor(state: UpdateState, current: string): UpdateStatus {
  if (!state.latest) return "never";
  return isNewer(state.latest, current) ? "newer" : "current";
}

/** Defensive: the store file is user-editable and may carry nothing at all. */
export function normalizeUpdateState(raw: unknown): UpdateState {
  const base = defaultUpdateState();
  if (!raw || typeof raw !== "object") return base;
  const r = raw as Partial<Record<keyof UpdateState, unknown>>;
  const version = (v: unknown) => (typeof v === "string" && parseVersion(v) ? v : null);
  return {
    enabled: r.enabled === true,
    asked: r.asked === true || typeof r.enabled === "boolean",
    checkedMs: typeof r.checkedMs === "number" && Number.isFinite(r.checkedMs) ? r.checkedMs : null,
    latest: version(r.latest),
    told: version(r.told),
  };
}
