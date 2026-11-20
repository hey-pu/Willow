import { localDayKey } from "./dates.ts";

export const RANGE_PRESETS = [
  "today", "yesterday", "24h", "7d", "14d", "30d", "90d",
  "thisMonth", "lastMonth", "all",
] as const;

export type RangePreset = (typeof RANGE_PRESETS)[number];

export type DateRange =
  | { kind: "preset"; preset: RangePreset }
  | { kind: "custom"; start: string; end: string };

type DateBounds = { sinceMs?: number; untilMs?: number };

const DAYS = { "7d": 7, "14d": 14, "30d": 30, "90d": 90 };
const DEFAULT_RANGE: DateRange = { kind: "preset", preset: "30d" };

function isPreset(value: unknown): value is RangePreset {
  return RANGE_PRESETS.some((preset) => preset === value);
}

function parseDay(value: string): Date | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return;
  const [, year, month, day] = match.map(Number);
  if (year < 1) return;
  // setFullYear handles years 0001–0099 literally (the Date constructor
  // would interpret them as 1901–1999).
  const date = new Date(0);
  date.setFullYear(year, month - 1, day);
  date.setHours(0, 0, 0, 0);
  if (date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day) {
    return date;
  }
}

export function isValidCustomRange(start: string, end: string): boolean {
  return start <= end && !!parseDay(start) && !!parseDay(end);
}

/** Accept previously saved bare preset IDs as well as the current object. */
export function readDateRange(saved: string | null): DateRange {
  if (isPreset(saved)) return { kind: "preset", preset: saved };
  if (!saved) return DEFAULT_RANGE;
  try {
    const range = JSON.parse(saved);
    if (range?.kind === "preset" && isPreset(range.preset)) {
      return { kind: "preset", preset: range.preset };
    }
    if (range?.kind === "custom" && typeof range.start === "string" &&
        typeof range.end === "string" && isValidCustomRange(range.start, range.end)) {
      return { kind: "custom", start: range.start, end: range.end };
    }
  } catch {
    // Fall back when a saved value is malformed.
  }
  return DEFAULT_RANGE;
}

/** Move on the local calendar before setting midnight to account for DST. */
function dayStart(now: Date, day: number, monthOffset = 0): number {
  const date = new Date(now);
  date.setMonth(date.getMonth() + monthOffset, day);
  return date.setHours(0, 0, 0, 0);
}

/** Inclusive lower bound and exclusive upper bound, matching SQLite. */
export function rangeBounds(range: DateRange, now = new Date()): DateBounds {
  if (range.kind === "custom") {
    const start = parseDay(range.start);
    const end = parseDay(range.end);
    if (!start || !end || start > end) return {};
    return { sinceMs: start.getTime(), untilMs: dayStart(end, end.getDate() + 1) };
  }

  const day = now.getDate();
  const tomorrow = dayStart(now, day + 1);
  switch (range.preset) {
    case "all":
      return {};
    case "today":
      return { sinceMs: dayStart(now, day), untilMs: tomorrow };
    case "yesterday":
      return { sinceMs: dayStart(now, day - 1), untilMs: dayStart(now, day) };
    case "24h": {
      const untilMs = now.getTime();
      return { sinceMs: untilMs - 24 * 60 * 60 * 1000, untilMs };
    }
    case "thisMonth":
      return { sinceMs: dayStart(now, 1), untilMs: tomorrow };
    case "lastMonth":
      return { sinceMs: dayStart(now, 1, -1), untilMs: dayStart(now, 1) };
    default:
      return { sinceMs: dayStart(now, day + 1 - DAYS[range.preset]), untilMs: tomorrow };
  }
}

export function isSingleLocalDay({ sinceMs, untilMs }: DateBounds): boolean {
  return sinceMs !== undefined && untilMs !== undefined && untilMs > sinceMs &&
    localDayKey(new Date(sinceMs)) === localDayKey(new Date(untilMs - 1));
}

export function rangeInputDates(range: DateRange, now = new Date()) {
  if (range.kind === "custom") return { start: range.start, end: range.end };
  const { sinceMs, untilMs } = rangeBounds(range, now);
  return {
    start: sinceMs === undefined ? "" : localDayKey(new Date(sinceMs)),
    end: untilMs === undefined ? "" : localDayKey(new Date(untilMs - 1)),
  };
}
