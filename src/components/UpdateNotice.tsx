import { getVersion } from "@tauri-apps/api/app";
import { ArrowDownToLine, ExternalLink, History, LoaderCircle } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useI18n } from "@/lib/i18n";
import { installSignedUpdate, useUpdateInstaller } from "@/lib/appUpdater";
import {
  checkForUpdate,
  checkOnStartup,
  dismissUpdate,
  getInstalledRelease,
  openRelease,
  useUpdates,
  type AppRelease,
} from "@/lib/updates";

const copy = {
  en: {
    updates: "Updates",
    installed: (version: string) => `Installed version · v${version}`,
    check: "Check for updates",
    checking: "Checking for updates…",
    latest: "You’re up to date.",
    failed: "Couldn’t check for updates. Check your connection and try again.",
    rateLimited: "GitHub has temporarily limited requests. Try again later.",
    noInstaller: "No published Windows x64 installer was found.",
    available: (version: string) => `Willow v${version} is available`,
    changes: "View changes",
    download: "Open download page",
    install: "Download and install",
    restartHint: "Willow will close and restart to complete installation.",
    installing: (percent: number | null) => percent === null ? "Downloading update…" : `Downloading update… ${percent}%`,
    verifying: "Verifying the downloaded update…",
    installingApp: "Installing update… Willow will restart.",
    installFailed: "The signed update could not be installed. You can download the installer instead.",
    later: "Later",
    whatsNew: "What’s new in this version",
    loadingNotes: "Loading release notes…",
    noNotes: "Release notes aren’t available for this version yet.",
    notesFailed: "Couldn’t load release notes.",
    retry: "Retry",
    openFailed: "Couldn’t open the release page.",
    releasePage: "View on GitHub",
    bundledNotes: [
      "Keep today’s token usage and estimated cost visible with the Windows desktop widget. Open the quick panel from the widget for more detail.",
      "Check for new Windows versions in Settings or open the download page from the update reminder.",
    ],
    updaterNotes: [
      "Download and install future Windows updates inside Willow, with progress and signature verification.",
      "Willow closes during installation and restarts when the update is complete. The full installer remains available on GitHub.",
    ],
  },
  zh: {
    updates: "版本更新",
    installed: (version: string) => `已安装版本 · v${version}`,
    check: "检查更新",
    checking: "正在检查更新…",
    latest: "当前已是最新版本。",
    failed: "无法检查更新。请检查网络连接后重试。",
    rateLimited: "GitHub 暂时限制了请求次数，请稍后重试。",
    noInstaller: "未找到已发布的 Windows x64 安装包。",
    available: (version: string) => `Willow v${version} 已发布`,
    changes: "查看变化",
    download: "打开下载页",
    install: "下载并安装",
    restartHint: "安装时 Willow 将退出并重新启动。",
    installing: (percent: number | null) => percent === null ? "正在下载更新…" : `正在下载更新… ${percent}%`,
    verifying: "正在验证更新文件…",
    installingApp: "正在安装更新，Willow 随后会重新启动。",
    installFailed: "无法安装签名更新，你可以改为下载完整安装包。",
    later: "稍后再看",
    whatsNew: "本次更新了什么",
    loadingNotes: "正在加载版本说明…",
    noNotes: "当前版本暂无发布说明。",
    notesFailed: "无法加载版本说明。",
    retry: "重试",
    openFailed: "无法打开发布页。",
    releasePage: "在 GitHub 查看",
    bundledNotes: [
      "Windows 桌面组件可常驻显示今日 Token 用量和估算费用，点击组件可打开快速面板查看更多信息。",
      "在设置中检查新的 Windows 版本，也可以从更新提醒直接打开下载页。",
    ],
    updaterNotes: [
      "支持在 Willow 内下载并安装后续 Windows 更新，显示下载进度并校验更新签名。",
      "安装时应用会退出，完成后自动重启，GitHub 也保留完整安装包下载入口。",
    ],
  },
} as const;

function ReleaseLink({ tag, label, primary = false, onOpened }: {
  tag: string | null;
  label: string;
  primary?: boolean;
  onOpened?: () => void;
}) {
  const { lang } = useI18n();
  const [status, setStatus] = useState<"idle" | "opening" | "failed">("idle");

  async function handleOpen() {
    setStatus("opening");
    try {
      await openRelease(tag);
      setStatus("idle");
      onOpened?.();
    } catch {
      setStatus("failed");
    }
  }

  return (
    <div className="flex flex-col items-start gap-1">
      <Button type="button" variant={primary ? "default" : "outline"} size="sm"
        disabled={status === "opening"} onClick={() => void handleOpen()}>
        <ExternalLink className="size-3.5" aria-hidden="true" />
        {label}
      </Button>
      {status === "failed" ? <span role="status" className="text-xs text-muted-foreground">{copy[lang].openFailed}</span> : null}
    </div>
  );
}

function InstallUpdateButton({ release }: { release: AppRelease }) {
  const { lang } = useI18n();
  const text = copy[lang];
  const updater = useUpdateInstaller();
  const busy = updater.phase !== "idle" && updater.phase !== "failed";
  let label: string = text.install;
  if (updater.phase === "checking") label = text.checking;
  if (updater.phase === "downloading") label = text.installing(updater.percent);
  if (updater.phase === "verifying") label = text.verifying;
  if (updater.phase === "installing") label = text.installingApp;

  return (
    <div className="flex max-w-sm flex-col items-start gap-1.5">
      <Button type="button" size="sm" disabled={busy}
        onClick={() => void installSignedUpdate(release.version)}>
        {busy ? <LoaderCircle className="size-3.5 motion-safe:animate-spin" aria-hidden="true" /> : <ArrowDownToLine className="size-3.5" aria-hidden="true" />}
        {label}
      </Button>
      <span role="status" className="text-xs text-muted-foreground">
        {updater.phase === "failed" ? text.installFailed : text.restartHint}
      </span>
      <ReleaseLink tag={release.tag} label={text.download} />
    </div>
  );
}

export function UpdateNotice() {
  const { lang } = useI18n();
  const text = copy[lang];
  const { check, dismissedVersion } = useUpdates();
  const [expandedTag, setExpandedTag] = useState<string | null>(null);
  const notesId = useId();

  useEffect(checkOnStartup, []);

  const release =
    check.status === "ready" && check.result.updateAvailable
      ? check.result.latestRelease
      : null;
  if (!release || release.version === dismissedVersion) return null;
  const expanded = expandedTag === release.tag;

  return (
    <div className="rounded-xl border border-primary/30 bg-primary/10 px-4 py-3">
      <div className="flex flex-wrap items-start justify-between gap-3 sm:items-center">
        <p role="status" className="flex min-w-0 items-center gap-2.5 text-sm font-medium">
          <ArrowDownToLine className="size-4 shrink-0" aria-hidden="true" />
          <span className="wrap-break-word">{text.available(release.version)}</span>
        </p>
        <div className="flex flex-wrap items-start gap-2">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-expanded={expanded}
            aria-controls={notesId}
            onClick={() => setExpandedTag(expanded ? null : release.tag)}
          >
            {text.changes}
          </Button>
          <InstallUpdateButton release={release} />
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => dismissUpdate(release.version)}
          >
            {text.later}
          </Button>
        </div>
      </div>
      <div id={notesId} hidden={!expanded} className="mt-3 border-t border-primary/20 pt-3">
        {expanded ? <ReleaseNotes notes={release.notes || text.noNotes} /> : null}
      </div>
    </div>
  );
}

function ReleaseNotes({ notes }: { notes: string }) {
  return (
    <div className="max-h-56 select-text overflow-auto wrap-break-word whitespace-pre-wrap text-sm leading-relaxed text-muted-foreground">
      {notes}
    </div>
  );
}

type InstalledNotes =
  | { status: "idle" | "loading" | "failed" }
  | { status: "ready"; release: AppRelease | null };

function InstalledReleaseNotes({ version }: { version: string | null }) {
  const { lang } = useI18n();
  const text = copy[lang];
  const bundled = version === "0.5.0" || version === "0.5.1";
  const [open, setOpen] = useState(false);
  const [notes, setNotes] = useState<InstalledNotes>({ status: "idle" });

  async function loadNotes() {
    setNotes({ status: "loading" });
    try {
      setNotes({ status: "ready", release: await getInstalledRelease() });
    } catch {
      setNotes({ status: "failed" });
    }
  }

  const remote = notes.status === "ready" ? notes.release : null;

  return (
    <details
      className="rounded-xl border border-border px-3 py-2.5"
      onToggle={(event) => {
        const expanded = event.currentTarget.open;
        setOpen(expanded);
        if (expanded && !bundled && notes.status === "idle") void loadNotes();
      }}
    >
      <summary className="cursor-pointer text-sm font-medium focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
        {text.whatsNew}
      </summary>
      {open ? (
        <div className="space-y-3 pt-3">
          {bundled ? (
            <ul className="list-disc space-y-1.5 ps-5 text-sm leading-relaxed text-muted-foreground">
              {(version === "0.5.1" ? text.updaterNotes : text.bundledNotes).map((note) => (
                <li key={note}>{note}</li>
              ))}
            </ul>
          ) : notes.status === "loading" ? (
            <p role="status" className="text-sm text-muted-foreground">{text.loadingNotes}</p>
          ) : notes.status === "failed" ? (
            <div className="flex items-center gap-3">
              <p role="status" className="text-sm text-muted-foreground">{text.notesFailed}</p>
              <Button type="button" variant="outline" size="sm" onClick={() => void loadNotes()}>
                {text.retry}
              </Button>
            </div>
          ) : notes.status === "ready" ? (
            <>
              <ReleaseNotes notes={remote?.notes || text.noNotes} />
              {remote ? <ReleaseLink tag={remote.tag} label={text.releasePage} /> : null}
            </>
          ) : null}
        </div>
      ) : null}
    </details>
  );
}

export function UpdateSettings() {
  const { lang } = useI18n();
  const text = copy[lang];
  const { check } = useUpdates();
  const updater = useUpdateInstaller();
  const [appVersion, setAppVersion] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getVersion().then(
      (version) => {
        if (!cancelled) setAppVersion(version);
      },
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const result = check.status === "ready" ? check.result : null;
  const version = result?.currentVersion ?? appVersion;
  const release = result?.updateAvailable ? result.latestRelease : null;
  let status: string | null = null;
  if (updater.phase === "checking") {
    status = text.checking;
  } else if (updater.phase === "downloading") {
    status = text.installing(updater.percent);
  } else if (updater.phase === "verifying") {
    status = text.verifying;
  } else if (updater.phase === "installing") {
    status = text.installingApp;
  } else if (updater.phase === "failed") {
    status = text.installFailed;
  } else if (check.status === "checking") {
    status = text.checking;
  } else if (check.status === "failed") {
    status = check.rateLimited ? text.rateLimited : text.failed;
  } else if (release) {
    status = text.available(release.version);
  } else if (result) {
    status = result.latestRelease ? text.latest : text.noInstaller;
  }

  return (
    <Card>
      <CardHeader className="flex-row items-center gap-2 space-y-0">
        <History className="size-4 text-muted-foreground" aria-hidden="true" />
        <CardTitle>{text.updates}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-sm text-muted-foreground">
            {version ? text.installed(version) : null}
          </span>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={check.status === "checking"}
            onClick={() => void checkForUpdate()}
          >
            {check.status === "checking" ? (
              <LoaderCircle
                className="size-3.5 motion-safe:animate-spin"
                aria-hidden="true"
              />
            ) : null}
            {text.check}
          </Button>
        </div>
        <p role="status" className="text-sm text-muted-foreground empty:hidden">
          {status}
        </p>
        {release ? (
          <div className="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-3">
            <ReleaseNotes notes={release.notes || text.noNotes} />
            <InstallUpdateButton release={release} />
          </div>
        ) : null}
        <InstalledReleaseNotes version={version} />
      </CardContent>
    </Card>
  );
}
