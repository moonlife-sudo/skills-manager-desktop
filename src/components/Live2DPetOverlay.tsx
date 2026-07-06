import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Sparkles } from "lucide-react";
import { cn } from "../utils";
import { usePetSettings } from "../hooks/usePetSettings";
import type { Live2DPetConfig } from "../hooks/usePetSettings";

type PixiAppHandle = {
  destroy: (removeView?: boolean, stageOptions?: boolean | { children?: boolean; texture?: boolean; baseTexture?: boolean }) => void;
  view?: unknown;
};

let cubismCorePromise: Promise<void> | null = null;

function modelUrl(path: string) {
  if (/^https?:\/\//i.test(path)) return path;
  if (path.startsWith("/")) return path;
  return convertFileSrc(path);
}

function loadCubismCore() {
  const win = window as unknown as { Live2DCubismCore?: unknown };
  if (win.Live2DCubismCore) return Promise.resolve();
  if (cubismCorePromise) return cubismCorePromise;

  cubismCorePromise = new Promise<void>((resolve, reject) => {
    const existing = document.querySelector<HTMLScriptElement>('script[data-live2d-cubism-core="true"]');
    if (existing) {
      existing.addEventListener("load", () => resolve(), { once: true });
      existing.addEventListener("error", () => reject(new Error("Failed to load Live2D Cubism Core")), { once: true });
      return;
    }

    const script = document.createElement("script");
    script.src = "/vendor/live2dcubismcore.min.js";
    script.async = true;
    script.dataset.live2dCubismCore = "true";
    script.onload = () => resolve();
    script.onerror = () => reject(new Error("Failed to load Live2D Cubism Core"));
    document.head.appendChild(script);
  });

  return cubismCorePromise;
}

function positionClass(position: Live2DPetConfig["position"]) {
  if (position === "sidebar") return "left-6 bottom-12";
  if (position === "bottom-left") return "left-8 bottom-8";
  if (position === "top-right") return "right-8 top-16";
  return "right-8 bottom-8";
}

function Live2DPet({ pet }: { pet: Live2DPetConfig }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let disposed = false;
    let app: PixiAppHandle | null = null;

    async function mount() {
      if (!hostRef.current || !pet.modelPath.trim()) {
        setFailed(Boolean(pet.modelPath.trim()));
        return;
      }
      setFailed(false);

      try {
        await loadCubismCore();
        const PIXI = await import("pixi.js");
        (window as unknown as { PIXI?: unknown }).PIXI = PIXI;
        const { Live2DModel } = await import("pixi-live2d-display/cubism4");
        if (disposed || !hostRef.current) return;

        const pixiApp = new PIXI.Application({
          width: 220,
          height: 280,
          antialias: true,
          autoDensity: true,
          backgroundAlpha: 0,
          resolution: window.devicePixelRatio || 1,
        });
        app = pixiApp;
        hostRef.current.replaceChildren(pixiApp.view as HTMLCanvasElement);

        const model = await Live2DModel.from(modelUrl(pet.modelPath));
        if (disposed) {
          pixiApp.destroy(true);
          return;
        }

        const fit = Math.min(180 / Math.max(model.width, 1), 250 / Math.max(model.height, 1));
        model.scale.set(fit);
        model.x = (pixiApp.screen.width - model.width) / 2;
        model.y = pixiApp.screen.height - model.height;
        pixiApp.stage.addChild(model);
      } catch (error) {
        console.warn("Failed to load Live2D pet", error);
        setFailed(true);
      }
    }

    void mount();
    return () => {
      disposed = true;
      if (app) {
        try {
          app.destroy(true);
        } catch {
          // Best-effort cleanup. Pixi may throw if initialization failed midway.
        }
      }
    };
  }, [pet.modelPath]);

  return (
    <div
      className={cn(
        "pointer-events-auto fixed z-30 h-[280px] w-[220px] overflow-hidden",
        positionClass(pet.position)
      )}
      style={{
        opacity: pet.opacity,
        transform: `translate(${pet.x}px, ${pet.y}px) scale(${pet.scale})`,
        transformOrigin: pet.position.includes("right") ? "bottom right" : "bottom left",
      }}
      title={pet.name}
    >
      <div ref={hostRef} className="h-full w-full" />
      {failed ? (
        <div className="absolute inset-x-4 bottom-4 rounded-lg border border-border-subtle bg-surface/90 px-3 py-2 text-center text-[12px] text-muted shadow-lg backdrop-blur">
          <Sparkles className="mx-auto mb-1 h-4 w-4 text-accent" />
          Live2D model unavailable
        </div>
      ) : null}
    </div>
  );
}

export function Live2DPetOverlay() {
  const { pets } = usePetSettings();
  const enabledPets = useMemo(
    () => pets.filter((pet) => pet.enabled && pet.modelPath.trim()),
    [pets]
  );

  if (enabledPets.length === 0) return null;

  return (
    <div className="pointer-events-none fixed inset-0 z-30">
      {enabledPets.map((pet) => (
        <Live2DPet key={pet.id} pet={pet} />
      ))}
    </div>
  );
}
