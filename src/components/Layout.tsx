import { useEffect, useMemo, useRef } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { StatusBanner } from "./StatusBanner";
import { CommandPalette } from "./CommandPalette";
import { GlobalSkillDetailHost } from "./GlobalSkillDetailHost";
import { Live2DPetOverlay } from "./Live2DPetOverlay";
import { useApp } from "../context/AppContext";
import { useThemeContext } from "../context/ThemeContext";
import { useTranslation } from "react-i18next";
import { useDragWindow } from "../hooks/useDragWindow";

function backgroundMediaSrc(path: string) {
  if (!path) return "";
  if (/^(https?:|data:|blob:|\/)/i.test(path)) return path;
  return convertFileSrc(path);
}

export function Layout() {
  const { t } = useTranslation();
  const { appError, refreshAppData } = useApp();
  const { appearance } = useThemeContext();
  const onDrag = useDragWindow();
  const navigate = useNavigate();
  const location = useLocation();
  const videoRef = useRef<HTMLVideoElement | null>(null);

  const backgroundVideoSrc = useMemo(
    () => appearance.backgroundKind === "video" ? backgroundMediaSrc(appearance.backgroundPath) : "",
    [appearance.backgroundKind, appearance.backgroundPath]
  );

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.playbackRate = appearance.videoPlaybackRate;
    video.volume = appearance.videoMuted ? 0 : appearance.videoVolume;
    video.muted = appearance.videoMuted;
    void video.play().catch(() => undefined);
  }, [appearance.videoMuted, appearance.videoPlaybackRate, appearance.videoVolume, backgroundVideoSrc]);

  // Cmd+, to open Settings
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        const target = e.target as HTMLElement;
        if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) return;
        e.preventDefault();
        navigate("/settings");
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        const target = e.target as HTMLElement;
        if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) return;
        e.preventDefault();
        refreshAppData();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [navigate, refreshAppData]);

  return (
    <div className="relative flex h-full w-full overflow-hidden bg-background text-primary">
      {appearance.backgroundKind === "video" && backgroundVideoSrc ? (
        <video
          key={backgroundVideoSrc}
          ref={videoRef}
          className="pointer-events-none absolute inset-0 h-full w-full object-cover"
          src={backgroundVideoSrc}
          autoPlay
          loop
          playsInline
          muted={appearance.videoMuted}
          style={{
            opacity: "var(--appearance-background-opacity)",
            filter: "blur(var(--appearance-background-blur))",
            transform: "scale(1.04)",
          }}
        />
      ) : (
        <div
          className="pointer-events-none absolute inset-0 bg-cover bg-center"
          style={{
            backgroundImage: "var(--appearance-background-image)",
            opacity: "var(--appearance-background-opacity)",
            filter: "blur(var(--appearance-background-blur))",
            transform: "scale(1.04)",
          }}
        />
      )}
      <div
        className="pointer-events-none absolute inset-0 bg-black"
        style={{ opacity: "var(--appearance-background-dim)" }}
      />
      {/* Full-width top drag bar — spans sidebar + content, with bottom divider */}
      <div
        onMouseDown={onDrag}
        className="app-drag-bar absolute inset-x-0 top-0 z-50 h-[28px] border-b border-border-subtle"
      />
      <Sidebar />
      <div className="relative z-10 flex min-w-[600px] flex-1 flex-col overflow-hidden">
        <div className="app-content-scrim pointer-events-none absolute inset-0" />
        <div className="relative flex-1 overflow-y-auto px-5 pb-5 pt-[calc(28px+20px)] scrollbar-hide">
          <div className="mx-auto flex min-h-full max-w-[1200px] flex-col gap-4">
            {appError ? (
              <StatusBanner
                compact
                title={t("common.dataOutOfDate")}
                description={appError}
                actionLabel={t("common.retry")}
                onAction={refreshAppData}
                tone="danger"
              />
            ) : null}
            <Outlet />
          </div>
        </div>
      </div>
      <CommandPalette />
      <Live2DPetOverlay />
      {location.pathname !== "/my-skills" ? <GlobalSkillDetailHost /> : null}
    </div>
  );
}
