export type ColorTheme = "light" | "dark";

const THEME_STORAGE_KEY = "iota-desktop-theme";

function isColorTheme(value: unknown): value is ColorTheme {
  return value === "light" || value === "dark";
}

export function getInitialTheme(): ColorTheme {
  const documentTheme = document.documentElement.dataset.theme;
  if (isColorTheme(documentTheme)) return documentTheme;

  try {
    const storedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (isColorTheme(storedTheme)) return storedTheme;
  } catch {
    // Storage may be unavailable in a restricted webview; use the system preference instead.
  }

  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

export function applyTheme(theme: ColorTheme, persist = true) {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;

  if (!persist) return;
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Applying the theme should still succeed when storage is unavailable.
  }
}
