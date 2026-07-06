import { useState, useEffect, useCallback } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import * as api from "../lib/tauri";

export type Theme = "light" | "dark" | "system" | "sakura" | "anime" | "cyber" | "soft";
export type ResolvedTheme = "light" | "dark";
export type ReadabilityMode = "balanced" | "readable" | "immersive";

export interface AppearanceSettings {
  backgroundKind: "image" | "video";
  backgroundPath: string;
  backgroundOpacity: number;
  backgroundBlur: number;
  backgroundDim: number;
  panelOpacity: number;
  videoPlaybackRate: number;
  videoMuted: boolean;
  videoVolume: number;
  readabilityMode: ReadabilityMode;
}

export type AppearanceKey = keyof AppearanceSettings;

const LEGACY_STORAGE_KEY = "theme";
const VARIANT_STORAGE_KEY = "theme_variant";
const APPEARANCE_STORAGE_PREFIX = "appearance.";

const APPEARANCE_SETTING_KEYS: Record<AppearanceKey, string> = {
  backgroundKind: "appearance_background_kind",
  backgroundPath: "appearance_background_path",
  backgroundOpacity: "appearance_background_opacity",
  backgroundBlur: "appearance_background_blur",
  backgroundDim: "appearance_background_dim",
  panelOpacity: "appearance_panel_opacity",
  videoPlaybackRate: "appearance_video_playback_rate",
  videoMuted: "appearance_video_muted",
  videoVolume: "appearance_video_volume",
  readabilityMode: "appearance_readability_mode",
};

const DEFAULT_APPEARANCE: AppearanceSettings = {
  backgroundKind: "image",
  backgroundPath: "",
  backgroundOpacity: 0.45,
  backgroundBlur: 0,
  backgroundDim: 0.08,
  panelOpacity: 0.88,
  videoPlaybackRate: 1,
  videoMuted: true,
  videoVolume: 0.5,
  readabilityMode: "balanced",
};

const READABILITY_TOKENS: Record<ReadabilityMode, {
  panelFloor: number;
  contentScrim: number;
  chromeScrim: number;
  headerScrim: number;
  glassBlur: number;
}> = {
  readable: {
    panelFloor: 0.96,
    contentScrim: 0.5,
    chromeScrim: 0.94,
    headerScrim: 0.92,
    glassBlur: 24,
  },
  balanced: {
    panelFloor: 0.92,
    contentScrim: 0.34,
    chromeScrim: 0.88,
    headerScrim: 0.86,
    glassBlur: 20,
  },
  immersive: {
    panelFloor: 0.82,
    contentScrim: 0.18,
    chromeScrim: 0.74,
    headerScrim: 0.68,
    glassBlur: 14,
  },
};

function isTheme(value: string | null): value is Theme {
  return (
    value === "light" ||
    value === "dark" ||
    value === "system" ||
    value === "sakura" ||
    value === "anime" ||
    value === "cyber" ||
    value === "soft"
  );
}

function getSystemTheme(): ResolvedTheme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function resolveTheme(theme: Theme): ResolvedTheme {
  if (theme === "system") return getSystemTheme();
  if (theme === "dark" || theme === "cyber") return "dark";
  return "light";
}

function clampNumber(value: string | number | null, fallback: number, min: number, max: number) {
  const numeric = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(numeric)) return fallback;
  return Math.min(max, Math.max(min, numeric));
}

function initialAppearance(): AppearanceSettings {
  const readNumber = (key: AppearanceKey, min: number, max: number) =>
    clampNumber(
      localStorage.getItem(`${APPEARANCE_STORAGE_PREFIX}${key}`),
      DEFAULT_APPEARANCE[key] as number,
      min,
      max
    );

  const storedPath =
    localStorage.getItem(`${APPEARANCE_STORAGE_PREFIX}backgroundPath`) ??
    DEFAULT_APPEARANCE.backgroundPath;
  const storedKind = localStorage.getItem(`${APPEARANCE_STORAGE_PREFIX}backgroundKind`);
  const storedReadabilityMode = localStorage.getItem(`${APPEARANCE_STORAGE_PREFIX}readabilityMode`);

  return {
    backgroundKind:
      storedKind === "video" || storedKind === "image"
        ? storedKind
        : /\.mp4$/i.test(storedPath)
          ? "video"
          : DEFAULT_APPEARANCE.backgroundKind,
    backgroundPath: storedPath,
    backgroundOpacity: readNumber("backgroundOpacity", 0, 1),
    backgroundBlur: readNumber("backgroundBlur", 0, 32),
    backgroundDim: readNumber("backgroundDim", 0, 0.8),
    panelOpacity: readNumber("panelOpacity", 0.72, 1),
    videoPlaybackRate: readNumber("videoPlaybackRate", 0.25, 3),
    videoMuted:
      (localStorage.getItem(`${APPEARANCE_STORAGE_PREFIX}videoMuted`) ?? String(DEFAULT_APPEARANCE.videoMuted)) === "true",
    videoVolume: readNumber("videoVolume", 0, 1),
    readabilityMode: isReadabilityMode(storedReadabilityMode)
      ? storedReadabilityMode
      : DEFAULT_APPEARANCE.readabilityMode,
  };
}

function applyThemeClass(theme: Theme, resolved: ResolvedTheme) {
  const root = document.documentElement;
  root.classList.remove("dark", "theme-sakura", "theme-anime", "theme-cyber", "theme-soft");
  if (resolved === "dark") {
    root.classList.add("dark");
  }
  if (theme !== "light" && theme !== "dark" && theme !== "system") {
    root.classList.add(`theme-${theme}`);
  }
}

function applyAppearance(appearance: AppearanceSettings) {
  const root = document.documentElement;
  const hasWallpaper = appearance.backgroundPath.trim().length > 0;
  const tokens = READABILITY_TOKENS[appearance.readabilityMode];
  const imageUrl = appearance.backgroundPath
    ? `url("${convertFileSrc(appearance.backgroundPath)}")`
    : "none";

  root.dataset.wallpaper = hasWallpaper ? "on" : "off";
  root.dataset.readability = appearance.readabilityMode;
  root.style.setProperty("--appearance-background-image", imageUrl);
  root.style.setProperty("--appearance-background-opacity", String(appearance.backgroundOpacity));
  root.style.setProperty("--appearance-background-blur", `${appearance.backgroundBlur}px`);
  root.style.setProperty("--appearance-background-dim", String(appearance.backgroundDim));
  root.style.setProperty("--appearance-panel-opacity", String(appearance.panelOpacity));
  root.style.setProperty("--appearance-panel-floor", String(hasWallpaper ? tokens.panelFloor : 0));
  root.style.setProperty("--appearance-content-scrim-opacity", String(hasWallpaper ? tokens.contentScrim : 0));
  root.style.setProperty("--appearance-chrome-scrim-opacity", String(hasWallpaper ? tokens.chromeScrim : appearance.panelOpacity));
  root.style.setProperty("--appearance-header-scrim-opacity", String(hasWallpaper ? tokens.headerScrim : 0));
  root.style.setProperty("--appearance-glass-blur", `${tokens.glassBlur}px`);
}

function isAppearanceBackgroundKind(value: string | null): value is AppearanceSettings["backgroundKind"] {
  return value === "image" || value === "video";
}

function isReadabilityMode(value: string | null): value is ReadabilityMode {
  return value === "balanced" || value === "readable" || value === "immersive";
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(() => {
    const stored = localStorage.getItem(VARIANT_STORAGE_KEY) ?? localStorage.getItem(LEGACY_STORAGE_KEY);
    return isTheme(stored) ? stored : "dark";
  });
  const [appearance, setAppearanceState] = useState<AppearanceSettings>(() => initialAppearance());

  const resolvedTheme = resolveTheme(theme);

  useEffect(() => {
    applyThemeClass(theme, resolvedTheme);
  }, [resolvedTheme, theme]);

  useEffect(() => {
    applyAppearance(appearance);
  }, [appearance]);

  useEffect(() => {
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = () => applyThemeClass("system", getSystemTheme());
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [theme]);

  useEffect(() => {
    Promise.all([
      api.getSettings("theme_variant"),
      api.getSettings("theme"),
    ]).then(([variant, legacy]) => {
      const next = isTheme(variant) ? variant : isTheme(legacy) ? legacy : null;
      if (next) {
        setThemeState(next);
        localStorage.setItem(VARIANT_STORAGE_KEY, next);
        localStorage.setItem(LEGACY_STORAGE_KEY, next);
      }
    });

    (Object.keys(APPEARANCE_SETTING_KEYS) as AppearanceKey[]).forEach((key) => {
      api.getSettings(APPEARANCE_SETTING_KEYS[key]).then((value) => {
        if (value == null) return;
        setAppearanceState((current) => {
          const next = { ...current };
          if (key === "backgroundKind") {
            next.backgroundKind = isAppearanceBackgroundKind(value) ? value : DEFAULT_APPEARANCE.backgroundKind;
          } else if (key === "backgroundPath") {
            next.backgroundPath = value;
          } else if (key === "backgroundBlur") {
            next.backgroundBlur = clampNumber(value, DEFAULT_APPEARANCE.backgroundBlur, 0, 32);
          } else if (key === "backgroundDim") {
            next.backgroundDim = clampNumber(value, DEFAULT_APPEARANCE.backgroundDim, 0, 0.8);
          } else if (key === "panelOpacity") {
            next.panelOpacity = clampNumber(value, DEFAULT_APPEARANCE.panelOpacity, 0.72, 1);
          } else if (key === "videoPlaybackRate") {
            next.videoPlaybackRate = clampNumber(value, DEFAULT_APPEARANCE.videoPlaybackRate, 0.25, 3);
          } else if (key === "videoMuted") {
            next.videoMuted = value === "true";
          } else if (key === "videoVolume") {
            next.videoVolume = clampNumber(value, DEFAULT_APPEARANCE.videoVolume, 0, 1);
          } else if (key === "readabilityMode") {
            next.readabilityMode = isReadabilityMode(value) ? value : DEFAULT_APPEARANCE.readabilityMode;
          } else {
            next.backgroundOpacity = clampNumber(value, DEFAULT_APPEARANCE.backgroundOpacity, 0, 1);
          }
          localStorage.setItem(`${APPEARANCE_STORAGE_PREFIX}${key}`, String(next[key]));
          return next;
        });
      });
    });
  }, []);

  const setTheme = useCallback((next: Theme) => {
    setThemeState(next);
    localStorage.setItem(VARIANT_STORAGE_KEY, next);
    localStorage.setItem(LEGACY_STORAGE_KEY, next);
    api.setSettings("theme_variant", next);
    api.setSettings("theme", next);
  }, []);

  const setAppearanceValue = useCallback(<K extends AppearanceKey>(key: K, value: AppearanceSettings[K]) => {
    setAppearanceState((current) => {
      const next = { ...current, [key]: value };
      localStorage.setItem(`${APPEARANCE_STORAGE_PREFIX}${key}`, String(value));
      api.setSettings(APPEARANCE_SETTING_KEYS[key], String(value));
      return next;
    });
  }, []);

  const resetAppearance = useCallback(() => {
    setAppearanceState(DEFAULT_APPEARANCE);
    (Object.keys(APPEARANCE_SETTING_KEYS) as AppearanceKey[]).forEach((key) => {
      const value = DEFAULT_APPEARANCE[key];
      localStorage.setItem(`${APPEARANCE_STORAGE_PREFIX}${key}`, String(value));
      api.setSettings(APPEARANCE_SETTING_KEYS[key], String(value));
    });
  }, []);

  return { theme, setTheme, resolvedTheme, appearance, setAppearanceValue, resetAppearance };
}
