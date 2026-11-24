export type InstallerState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "downloading"; percent: number | null }
  | { phase: "verifying" | "installing" }
  | { phase: "failed" };

type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

export interface SignedUpdate {
  version: string;
  download(onEvent: (event: DownloadEvent) => void, options: { timeout: number }): Promise<void>;
  install(options: { restartAfterInstall: boolean }): Promise<void>;
  close(): Promise<void>;
}

export type InstallResult = "installed" | "unavailable" | "failed";

/** One install operation shared by the banner and Settings page. */
export function createUpdateInstaller(check: () => Promise<SignedUpdate | null>) {
  let state: InstallerState = { phase: "idle" };
  let busy = false;
  const listeners = new Set<() => void>();

  function publish(next: InstallerState) {
    state = next;
    listeners.forEach((listener) => listener());
  }

  return {
    getSnapshot: () => state,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    async install(expectedVersion: string): Promise<InstallResult> {
      if (busy) return "failed";
      busy = true;
      publish({ phase: "checking" });
      let update: SignedUpdate | null = null;
      let result: InstallResult = "failed";
      try {
        update = await check();
        // Bind the user's install action to the release they reviewed. A
        // stale manifest must never install a different release silently.
        if (!update) {
          result = "unavailable";
          publish({ phase: "failed" });
          return result;
        }
        if (update.version !== expectedVersion) {
          throw new Error("The signed update differs from the selected release");
        }
        let downloaded = 0;
        let total: number | undefined;
        publish({ phase: "downloading", percent: null });
        await update.download((event) => {
          if (event.event === "Started") {
            total = event.data.contentLength;
            publish({ phase: "downloading", percent: total ? 0 : null });
          } else if (event.event === "Progress") {
            downloaded += event.data.chunkLength;
            publish({
              phase: "downloading",
              percent: total ? Math.min(100, Math.round(downloaded / total * 100)) : null,
            });
          } else {
            publish({ phase: "verifying" });
          }
        }, { timeout: 120_000 });
        publish({ phase: "installing" });
        await update.install({ restartAfterInstall: true });
        result = "installed";
        // The Windows plugin exits the app after launching the installer.
      } catch (error) {
        console.error("signed Willow update failed:", error);
        publish({ phase: "failed" });
      } finally {
        if (update) await update.close().catch(() => {});
        busy = false;
      }
      return result;
    },
  };
}
