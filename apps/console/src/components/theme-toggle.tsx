import { useState } from "react";

import { Button } from "@/components/ui/button";
import { getTheme, setTheme, type Theme } from "@/lib/theme";

const ORDER: Theme[] = ["system", "light", "dark"];
const LABEL: Record<Theme, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};

/** Cycles system -> light -> dark. Choice is remembered per browser. */
export function ThemeToggle() {
  const [theme, setThemeState] = useState<Theme>(() => getTheme());

  function cycle() {
    const next = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length];
    setTheme(next);
    setThemeState(next);
  }

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={cycle}
      title={`Theme: ${LABEL[theme]}. Click to change.`}
    >
      {LABEL[theme]}
    </Button>
  );
}
