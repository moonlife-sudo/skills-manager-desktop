/* eslint-disable react-refresh/only-export-components */
import { createContext, useContext } from "react";
import {
  useTheme,
  type AppearanceKey,
  type AppearanceSettings,
  type Theme,
  type ResolvedTheme,
} from "../hooks/useTheme";

interface ThemeContextValue {
  theme: Theme;
  setTheme: (theme: Theme) => void;
  resolvedTheme: ResolvedTheme;
  appearance: AppearanceSettings;
  setAppearanceValue: <K extends AppearanceKey>(key: K, value: AppearanceSettings[K]) => void;
  resetAppearance: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const value = useTheme();
  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useThemeContext() {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useThemeContext must be used within ThemeProvider");
  return ctx;
}
