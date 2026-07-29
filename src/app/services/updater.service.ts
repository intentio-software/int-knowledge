import { Injectable, signal } from "@angular/core";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current"; version: string }
  | { kind: "available"; version: string; notes?: string; update: unknown }
  | { kind: "downloading" }
  | { kind: "failed"; message: string }
  | { kind: "unsupported" };

/**
 * Update checking for the desktop app.
 *
 * Mirrors Intentio Mind Map's updater, but reports through a signal instead of
 * toasts so the About dialog can show the result inline — one place to look
 * rather than a notification that disappears.
 */
@Injectable({ providedIn: "root" })
export class UpdaterService {
  readonly state = signal<UpdateState>({ kind: "idle" });

  get busy(): boolean {
    const kind = this.state().kind;
    return kind === "checking" || kind === "downloading";
  }

  /** Check for an update, reporting every outcome through `state`. */
  async check(): Promise<void> {
    if (!isDesktop()) {
      this.state.set({ kind: "unsupported" });
      return;
    }

    this.state.set({ kind: "checking" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (!update?.available) {
        this.state.set({ kind: "current", version: await currentVersion() });
        return;
      }
      this.state.set({
        kind: "available",
        version: update.version,
        notes: update.body,
        update
      });
    } catch (error) {
      this.state.set({ kind: "failed", message: describe(error) });
    }
  }

  /** Download and install a found update, then restart. */
  async install(): Promise<void> {
    const current = this.state();
    if (current.kind !== "available") {
      return;
    }
    this.state.set({ kind: "downloading" });
    try {
      await (current.update as { downloadAndInstall: () => Promise<void> }).downloadAndInstall();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (error) {
      this.state.set({ kind: "failed", message: describe(error) });
    }
  }

  reset(): void {
    this.state.set({ kind: "idle" });
  }
}

function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function currentVersion(): Promise<string> {
  try {
    const { getVersion } = await import("@tauri-apps/api/app");
    return await getVersion();
  } catch {
    return "";
  }
}

/**
 * Turn an update error into something a person can act on.
 *
 * Before any release exists the endpoint 404s, which is not a fault worth
 * alarming anyone about — it just means there is nothing published yet.
 */
function describe(error: unknown): string {
  const text = String(error);
  const lowered = text.toLowerCase();
  if (lowered.includes("404") || lowered.includes("not found")) {
    return "No releases have been published yet.";
  }
  if (lowered.includes("fetch") || lowered.includes("network") || lowered.includes("dns")) {
    return "Could not reach the update server. Try again later.";
  }
  if (lowered.includes("signature")) {
    return "The update failed its signature check and was not installed.";
  }
  return text;
}
