export const THEME_KEY = "on-n-off.theme";

export type Theme = "dark" | "light";

export function readTheme(): Theme {
  return localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark";
}

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  root.dataset.theme = "onnoff";
  root.classList.toggle("dark", theme === "dark");
  root.style.colorScheme = theme;
  localStorage.setItem(THEME_KEY, theme);
}

export function applyStoredTheme() {
  applyTheme(readTheme());
}
