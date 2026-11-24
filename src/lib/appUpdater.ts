import { useSyncExternalStore } from "react";
import { IN_TAURI } from "./api";
import { createUpdateInstaller } from "./updateInstaller";

const installer = createUpdateInstaller(async () => {
  if (!IN_TAURI) return null;
  const { check } = await import("@tauri-apps/plugin-updater");
  return check({ timeout: 12_000 });
});

export const installSignedUpdate = installer.install;

export function useUpdateInstaller() {
  return useSyncExternalStore(installer.subscribe, installer.getSnapshot);
}
