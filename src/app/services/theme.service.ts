import { Injectable, signal } from "@angular/core";

export type ThemePreference = "system" | "dark" | "light";

const STORAGE_KEY = "intentio-knowledge:theme";
const CYCLE: ThemePreference[] = ["system", "dark", "light"];

/**
 * Light/dark handling for the whole app.
 *
 * The resolved theme is written to `data-theme` on `<body>`; every colour in the
 * app — including the CodeMirror theme — reads from CSS custom properties keyed
 * off that attribute, so switching costs one attribute write.
 */
@Injectable({ providedIn: "root" })
export class ThemeService {
  readonly preference = signal<ThemePreference>(this.load());
  readonly resolved = signal<"dark" | "light">("dark");

  private media: MediaQueryList | null = null;

  constructor() {
    if (typeof window !== "undefined" && window.matchMedia) {
      this.media = window.matchMedia("(prefers-color-scheme: light)");
      // Following the OS only matters while the preference is "system", but the
      // listener is cheap and avoids re-subscribing on every toggle.
      this.media.addEventListener("change", () => this.apply());
    }
    this.apply();
  }

  cycle(): void {
    const next = CYCLE[(CYCLE.indexOf(this.preference()) + 1) % CYCLE.length];
    this.preference.set(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // A locked-down storage layer only costs us persistence.
    }
    this.apply();
  }

  label(): string {
    switch (this.preference()) {
      case "dark":
        return "Dark";
      case "light":
        return "Light";
      default:
        return "System";
    }
  }

  icon(): string {
    switch (this.preference()) {
      case "dark":
        return "pi-moon";
      case "light":
        return "pi-sun";
      default:
        return "pi-desktop";
    }
  }

  private apply(): void {
    const preference = this.preference();
    const systemLight = this.media?.matches ?? false;
    const resolved = preference === "system" ? (systemLight ? "light" : "dark") : preference;
    this.resolved.set(resolved);
    if (typeof document !== "undefined") {
      document.body.dataset["theme"] = resolved;
    }
  }

  private load(): ThemePreference {
    try {
      const stored = window.localStorage.getItem(STORAGE_KEY);
      if (stored === "dark" || stored === "light" || stored === "system") {
        return stored;
      }
    } catch {
      // Fall through to the default.
    }
    return "system";
  }
}
