const THEME_KEY = "berth.theme";

export type Theme = "system" | "light" | "dark";

export function getTheme(): Theme {
  const raw = localStorage.getItem(THEME_KEY);
  if (raw === "light" || raw === "dark" || raw === "system") {
    return raw;
  }
  return "system";
}

function prefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function applyTheme(theme: Theme): void {
  const dark = theme === "dark" || (theme === "system" && prefersDark());
  document.documentElement.classList.toggle("dark", dark);
}

export function initTheme(): void {
  applyTheme(getTheme());
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => {
      if (getTheme() === "system") {
        applyTheme("system");
      }
    });
}

export function setTheme(theme: Theme): void {
  localStorage.setItem(THEME_KEY, theme);
  applyTheme(theme);
}
