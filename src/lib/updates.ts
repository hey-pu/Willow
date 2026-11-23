import { invoke } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";

export interface AppRelease {
  tag: string;
  version: string;
  notes: string;
}

interface UpdateCheck {
  currentVersion: string;
  latestRelease: AppRelease | null;
  updateAvailable: boolean;
}

type UpdateStatus =
  | { status: "idle" | "checking" }
  | { status: "ready"; result: UpdateCheck }
  | { status: "failed"; rateLimited: boolean };

interface UpdateState {
  check: UpdateStatus;
  dismissedVersion: string | null;
}

const DISMISSED_KEY = "willow-dismissed-update";
const listeners = new Set<() => void>();

function readDismissedVersion(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

let state: UpdateState = {
  check: { status: "idle" },
  dismissedVersion: readDismissedVersion(),
};

function publish(patch: Partial<UpdateState>) {
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot() {
  return state;
}

export function useUpdates() {
  return useSyncExternalStore(subscribe, getSnapshot);
}

export async function checkForUpdate() {
  if (state.check.status === "checking") return;
  publish({ check: { status: "checking" } });
  try {
    const result = await invoke<UpdateCheck>("check_for_app_update");
    publish({ check: { status: "ready", result } });
  } catch (error) {
    publish({ check: { status: "failed", rateLimited: error === "rateLimited" } });
  }
}

export function checkOnStartup() {
  if (state.check.status === "idle") void checkForUpdate();
}

export function dismissUpdate(version: string) {
  try {
    localStorage.setItem(DISMISSED_KEY, version);
  } catch {
    // Dismissal still applies to the current session if storage is unavailable.
  }
  publish({ dismissedVersion: version });
}

export function getInstalledRelease() {
  return invoke<AppRelease | null>("get_app_release");
}

export function openRelease(tag: string | null) {
  return invoke<void>("open_app_release", { tag });
}
