import { useEffect, useId, useRef, useState } from "react";
import { ArrowRight, CalendarDays, ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import {
  isValidCustomRange,
  rangeInputDates,
  type DateRange,
  type RangePreset,
} from "@/lib/timeRange";
import { cn } from "@/lib/utils";

const PRESET_ROWS: readonly (readonly RangePreset[])[] = [
  ["today", "yesterday"],
  ["24h", "7d"],
  ["14d", "30d"],
  ["thisMonth", "lastMonth"],
  ["90d", "all"],
];

const labels = {
  en: {
    title: "Date range",
    today: "Today",
    yesterday: "Yesterday",
    "24h": "Last 24 hours",
    "7d": "Last 7 days",
    "14d": "Last 14 days",
    "30d": "Last 30 days",
    "90d": "Last 90 days",
    thisMonth: "This Month",
    lastMonth: "Last Month",
    all: "All Time",
    start: "Start date",
    end: "End date",
    apply: "Apply",
    invalid: "Enter a valid start and end date in order.",
  },
  zh: {
    title: "日期范围",
    today: "今天",
    yesterday: "昨天",
    "24h": "最近 24 小时",
    "7d": "最近 7 天",
    "14d": "最近 14 天",
    "30d": "最近 30 天",
    "90d": "最近 90 天",
    thisMonth: "本月",
    lastMonth: "上月",
    all: "全部时间",
    start: "开始日期",
    end: "结束日期",
    apply: "应用",
    invalid: "请输入有效日期，结束日期不得早于开始日期。",
  },
} as const;

type Draft = {
  preset: RangePreset | null;
  start: string;
  end: string;
};

function makeDraft(range: DateRange): Draft {
  return {
    preset: range.kind === "preset" ? range.preset : null,
    ...rangeInputDates(range),
  };
}

const dateInputClass =
  "h-8 w-full min-w-0 rounded-lg border border-border bg-background px-2.5 text-xs text-foreground tabular-nums outline-none focus-visible:ring-1 focus-visible:ring-ring";

export function DateRangeControl({
  value,
  onApply,
}: {
  value: DateRange;
  onApply: (range: DateRange) => void;
}) {
  const { lang } = useI18n();
  const copy = labels[lang];
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const open = draft !== null;

  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>("[data-initial-focus]")?.focus();
    const clickOutside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setDraft(null);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      setDraft(null);
      trigger.current?.focus();
    };
    document.addEventListener("pointerdown", clickOutside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", clickOutside);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  const summary = value.kind === "preset"
    ? copy[value.preset]
    : value.start === value.end
      ? value.start
      : `${value.start} – ${value.end}`;
  const invalid = draft !== null && draft.preset === null &&
    !isValidCustomRange(draft.start, draft.end);

  function apply() {
    if (!draft || invalid) return;
    onApply(draft.preset
      ? { kind: "preset", preset: draft.preset }
      : { kind: "custom", start: draft.start, end: draft.end });
    setDraft(null);
    trigger.current?.focus();
  }

  return (
    <div className="relative" ref={root}>
      <Button
        ref={trigger}
        type="button"
        variant="outline"
        size="sm"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={`${copy.title}: ${summary}`}
        onClick={() => setDraft(open ? null : makeDraft(value))}
      >
        <CalendarDays aria-hidden="true" className="h-3.5 w-3.5 text-muted-foreground" />
        <span className="tabular-nums">{summary}</span>
        <ChevronDown aria-hidden="true" className="h-3 w-3 text-muted-foreground" />
      </Button>
      {draft && (
        <form
          id={id}
          role="dialog"
          aria-label={copy.title}
          onSubmit={(event) => { event.preventDefault(); apply(); }}
          className="absolute right-0 top-full z-30 mt-2 w-[21rem] max-w-[calc(100vw-2rem)] space-y-3 rounded-xl border border-border bg-card p-3 shadow-lg"
        >
          <div className="grid grid-cols-2 gap-1.5">
            {PRESET_ROWS.flat().map((preset) => (
              <Button
                key={preset}
                type="button"
                size="sm"
                variant="outline"
                data-initial-focus={value.kind === "preset" && value.preset === preset ? "" : undefined}
                aria-pressed={draft.preset === preset}
                className={cn(
                  "justify-start text-left",
                  draft.preset === preset && "border-primary/40 bg-primary/10 text-primary hover:bg-primary/15",
                )}
                onClick={() => setDraft(makeDraft({ kind: "preset", preset }))}
              >
                {copy[preset]}
              </Button>
            ))}
          </div>
          <div className="grid grid-cols-[minmax(0,1fr)_1rem_minmax(0,1fr)] items-end gap-1.5 border-t border-border pt-3">
            <label className="min-w-0 space-y-1 text-xs text-muted-foreground">
              <span>{copy.start}</span>
              <input
                type="date"
                value={draft.start}
                data-initial-focus={value.kind === "custom" ? "" : undefined}
                aria-invalid={invalid || undefined}
                aria-describedby={invalid ? `${id}-error` : undefined}
                className={dateInputClass}
                onChange={(event) => setDraft({ preset: null, start: event.target.value, end: draft.end })}
              />
            </label>
            <ArrowRight className="mb-2.5 h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
            <label className="min-w-0 space-y-1 text-xs text-muted-foreground">
              <span>{copy.end}</span>
              <input
                type="date"
                value={draft.end}
                aria-invalid={invalid || undefined}
                aria-describedby={invalid ? `${id}-error` : undefined}
                className={dateInputClass}
                onChange={(event) => setDraft({ preset: null, start: draft.start, end: event.target.value })}
              />
            </label>
          </div>
          {invalid && <p id={`${id}-error`} className="text-xs text-red-500">{copy.invalid}</p>}
          <div className="flex justify-end">
            <Button size="sm" type="submit" disabled={invalid}>
              {copy.apply}
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}
