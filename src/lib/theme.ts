export type ThemeMode = "light" | "dark" | "system";

export const THEME_KEY = "voxmd-theme";

/** Kept in sync with the inline script in index.html that applies the theme
 *  before first paint. Changing the key or the values requires editing both. */
export function readStoredTheme(): ThemeMode {
  try {
    const saved = localStorage.getItem(THEME_KEY);
    if (saved === "light" || saved === "dark" || saved === "system") return saved;
  } catch {
    /* localStorage can throw in restricted contexts; fall through to the default */
  }
  return "system";
}

export function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function resolveTheme(mode: ThemeMode): "light" | "dark" {
  return mode === "system" ? (systemPrefersDark() ? "dark" : "light") : mode;
}
