import { useEffect, useState } from "react";
import { readStoredTheme, resolveTheme, THEME_KEY, type ThemeMode } from "../lib/theme";

/**
 * The initial value is also applied by an inline script in index.html so the
 * window does not flash the wrong theme before React mounts.
 */
export function useTheme(): [ThemeMode, (m: ThemeMode) => void] {
  const [themeMode, setThemeMode] = useState<ThemeMode>(readStoredTheme);

  useEffect(() => {
    try {
      localStorage.setItem(THEME_KEY, themeMode);
    } catch {
      /* non-fatal: the theme just will not persist */
    }

    const apply = () => document.documentElement.setAttribute("data-theme", resolveTheme(themeMode));
    apply();

    if (themeMode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [themeMode]);

  return [themeMode, setThemeMode];
}
