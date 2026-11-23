import { useRef, useState, type PointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronDown } from "lucide-react";
import { Logo } from "@/components/Logo";
import { IN_TAURI } from "@/lib/api";
import { formatCost, formatNumber } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { formatTokenCount } from "@/lib/tokenFormat";
import { useTokenUnits } from "@/lib/tokenUnits";
import { useTodayStats } from "@/lib/useTodayStats";
import { QuickPanel } from "./QuickPanel";

export function UsageWidget() {
  const { t } = useI18n();
  const { unit } = useTokenUnits();
  const { stats, error } = useTodayStats({
    activeBlock: false,
    pollMs: 60_000,
    pauseWhileHidden: true,
  });
  const [previewOpen, setPreviewOpen] = useState(false);
  const [openFailed, setOpenFailed] = useState(false);
  const ballPointer = useRef<{ id: number; x: number; y: number } | null>(null);
  const ballDragged = useRef(false);
  const tokens = stats ? formatTokenCount(stats.todayTokens, unit) : "—";
  const compactDigits = tokens.split(".")[0].replace(/[万亿KMB]$/, "").length >= 3 ? 0 : 1;
  const compact = stats ? formatTokenCount(stats.todayTokens, unit, compactDigits) : "—";
  const compactUnit = compact.match(/[万亿KMB]$/)?.[0] ?? "";
  const compactNumber = compactUnit ? compact.slice(0, -1) : compact;
  const cost = stats ? formatCost(stats.todayCost) : "—";
  const summary = stats
    ? `${t("quick.todayTokens")}: ${formatNumber(stats.todayTokens)}, ${t("quick.todayCost")}: ${cost}`
    : t("quick.todayTokens");
  const detailLabel = `${t("widget.open")}. ${summary}`;
  const failure = openFailed ? t("widget.openFailed") : error ? t("common.loadFailed") : null;

  async function openDetails() {
    if (!IN_TAURI) {
      setPreviewOpen(true);
      return;
    }
    setOpenFailed(false);
    try {
      await invoke("show_widget_details");
    } catch (e) {
      console.error("opening the usage widget details failed:", e);
      setOpenFailed(true);
    }
  }

  function startBallGesture(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0 || !event.isPrimary) return;
    ballDragged.current = false;
    ballPointer.current = { id: event.pointerId, x: event.clientX, y: event.clientY };
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function moveBallGesture(event: PointerEvent<HTMLButtonElement>) {
    const pointer = ballPointer.current;
    if (!pointer || pointer.id !== event.pointerId) return;
    if (Math.hypot(event.clientX - pointer.x, event.clientY - pointer.y) < 5) return;

    ballDragged.current = true;
    ballPointer.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
    if (IN_TAURI) {
      void getCurrentWindow().startDragging().catch((error) => {
        console.error("dragging the usage widget failed:", error);
      });
    }
  }

  function endBallGesture(event: PointerEvent<HTMLButtonElement>) {
    if (ballPointer.current?.id === event.pointerId) ballPointer.current = null;
  }

  return (
    <>
      <div className="willow-widget-shell">
        <div className="willow-widget-bar">
          <div data-tauri-drag-region title={t("widget.drag")} className="willow-widget-handle">
            <Logo size={29} bare />
          </div>
          <button
            type="button"
            onClick={() => void openDetails()}
            aria-label={detailLabel}
            aria-expanded={IN_TAURI ? undefined : previewOpen}
            title={failure ?? summary}
            className="willow-widget-open"
          >
            <span className="willow-widget-today">{t("quick.todayTokens")}</span>
            <span className="willow-widget-value">{tokens}</span>
            <span className="willow-widget-cost">{cost}</span>
            {failure ? <span className="willow-widget-alert" aria-hidden="true">!</span> : <ChevronDown aria-hidden="true" size={13} />}
          </button>
        </div>
        <button
          type="button"
          className="willow-widget-ball"
          onPointerDown={startBallGesture}
          onPointerMove={moveBallGesture}
          onPointerUp={endBallGesture}
          onPointerCancel={endBallGesture}
          onLostPointerCapture={endBallGesture}
          onClick={(event) => {
            // Keyboard activation remains available after a native drag.
            if (event.detail === 0 || !ballDragged.current) void openDetails();
          }}
          aria-label={detailLabel}
          aria-expanded={IN_TAURI ? undefined : previewOpen}
          title={failure ?? summary}
        >
          <span className="willow-widget-ball-handle">
            <Logo size={26} bare />
          </span>
          <span className="willow-widget-ball-open">
            <span className="willow-widget-ball-value" data-long={compact.length > 5}>
              {compactNumber}{compactUnit && <small>{compactUnit}</small>}
            </span>
            {failure && <span className="willow-widget-ball-alert" aria-hidden="true" />}
          </span>
        </button>
        {failure && <span role="status" className="sr-only">{failure}</span>}
      </div>
      {!IN_TAURI && previewOpen && (
        <div className="relative w-[360px] max-w-full">
          <button type="button" onClick={() => setPreviewOpen(false)} className="my-2 rounded-md border border-border bg-card px-2 py-1 text-xs">
            {t("widget.close")}
          </button>
          <div className="h-[460px] overflow-auto"><QuickPanel /></div>
        </div>
      )}
    </>
  );
}
