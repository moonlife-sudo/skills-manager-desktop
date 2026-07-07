import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { cn } from "../utils";
import {
  usePetSettings,
  type PetAction,
  type SpritePetConfig,
} from "../hooks/usePetSettings";

const FRAME_COUNT = 6;
const RANDOM_ACTIONS: PetAction[] = [
  "idle",
  "blink",
  "wave",
  "happy",
  "thinking",
  "typing",
  "surprised",
  "sleepy",
  "sad",
  "celebrate",
];

function normalizeBasePath(path: string) {
  return path.trim().replace(/[\\/]+$/, "");
}

function frameSrc(basePath: string, action: PetAction, frame: number, useLegacyFrame: boolean) {
  const base = normalizeBasePath(basePath || "/pets/academy-assistant");
  const relative = useLegacyFrame
    ? `${base}/${action}.png`
    : `${base}/${action}/frame-${String(frame).padStart(2, "0")}.png`;
  if (/^(https?:|data:|blob:|\/)/i.test(relative)) return relative;
  return convertFileSrc(relative.replace(/\//g, "\\"));
}

function clampToViewport(x: number, y: number) {
  const width = window.innerWidth || 1280;
  const height = window.innerHeight || 720;
  return {
    x: Math.max(0, Math.min(width - 96, x)),
    y: Math.max(28, Math.min(height - 96, y)),
  };
}

function randomPetAction(current: PetAction) {
  const candidates = RANDOM_ACTIONS.filter((action) => action !== current);
  return candidates[Math.floor(Math.random() * candidates.length)] ?? "idle";
}

function SpritePet({
  pet,
  updatePetOffset,
}: {
  pet: SpritePetConfig;
  updatePetOffset: (id: string, x: number, y: number) => void;
}) {
  const [temporaryAction, setTemporaryAction] = useState<PetAction | null>(null);
  const [temporaryOffset, setTemporaryOffset] = useState<{ x: number; y: number } | null>(null);
  const [currentFrame, setCurrentFrame] = useState(0);
  const [useLegacyFrame, setUseLegacyFrame] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const dragRef = useRef<{
    pointerId: number;
    startClientX: number;
    startClientY: number;
    startX: number;
    startY: number;
    moved: boolean;
  } | null>(null);
  const timerRef = useRef<number | null>(null);
  const randomTimerRef = useRef<number | null>(null);

  const displayedAction = temporaryAction ?? pet.idleAction;
  const displayedOffset = temporaryOffset ?? { x: pet.x, y: pet.y };
  const imageSrc = useMemo(
    () => frameSrc(pet.assetBasePath, displayedAction, currentFrame % FRAME_COUNT, useLegacyFrame),
    [currentFrame, displayedAction, pet.assetBasePath, useLegacyFrame]
  );

  useEffect(() => () => {
    if (timerRef.current != null) window.clearTimeout(timerRef.current);
    if (randomTimerRef.current != null) window.clearTimeout(randomTimerRef.current);
  }, []);

  useEffect(() => {
    const intervalMs = Math.max(42, Math.round(1000 / pet.animationFps));
    const interval = window.setInterval(() => {
      setCurrentFrame((frame) => (frame + 1) % FRAME_COUNT);
    }, intervalMs);
    return () => window.clearInterval(interval);
  }, [pet.animationFps]);

  useEffect(() => {
    if (!pet.randomActions) return;
    const schedule = () => {
      randomTimerRef.current = window.setTimeout(() => {
        if (dragRef.current) {
          schedule();
          return;
        }
        const nextAction = randomPetAction(displayedAction);
        setTemporaryAction(nextAction);
        if (timerRef.current != null) window.clearTimeout(timerRef.current);
        timerRef.current = window.setTimeout(() => {
          setTemporaryAction(null);
          timerRef.current = null;
        }, Math.max(1400, Math.round(1000 * Math.min(4, pet.randomActionIntervalSec * 0.45))));
        schedule();
      }, Math.max(4000, pet.randomActionIntervalSec * 1000));
    };
    schedule();
    return () => {
      if (randomTimerRef.current != null) window.clearTimeout(randomTimerRef.current);
    };
  }, [displayedAction, pet.randomActionIntervalSec, pet.randomActions]);

  const triggerAction = (nextAction: PetAction, duration = 1400) => {
    if (timerRef.current != null) window.clearTimeout(timerRef.current);
    setTemporaryAction(nextAction);
    timerRef.current = window.setTimeout(() => {
      setTemporaryAction(null);
      timerRef.current = null;
    }, duration);
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    dragRef.current = {
      pointerId: event.pointerId,
      startClientX: event.clientX,
      startClientY: event.clientY,
      startX: displayedOffset.x,
      startY: displayedOffset.y,
      moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    setTemporaryAction("drag");
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const dx = event.clientX - drag.startClientX;
    const dy = event.clientY - drag.startClientY;
    const moved = Math.abs(dx) + Math.abs(dy) > 4;
    drag.moved = drag.moved || moved;
    setTemporaryOffset(clampToViewport(drag.startX + dx, drag.startY + dy));
  };

  const finishPointer = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const finalOffset = clampToViewport(
      drag.startX + event.clientX - drag.startClientX,
      drag.startY + event.clientY - drag.startClientY
    );
    event.currentTarget.releasePointerCapture(event.pointerId);
    dragRef.current = null;
    if (drag.moved) {
      updatePetOffset(pet.id, finalOffset.x, finalOffset.y);
      setTemporaryOffset(null);
      setTemporaryAction(null);
      return;
    }
    setTemporaryOffset(null);
    triggerAction(pet.clickAction);
  };

  return (
    <div
      className={cn(
        "fixed left-0 top-0 z-40 select-none cursor-grab active:cursor-grabbing"
      )}
      style={{
        transform: `translate(${displayedOffset.x}px, ${displayedOffset.y}px) scale(${pet.scale})`,
        transformOrigin: "top left",
        opacity: pet.opacity,
      }}
      title={pet.name}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={finishPointer}
      onPointerCancel={finishPointer}
      onDoubleClick={() => triggerAction("celebrate", 1800)}
    >
      <div className="sprite-pet-shell">
        {loadFailed ? (
          <div className="rounded-lg border border-border-subtle bg-surface px-3 py-2 text-[12px] text-muted shadow-sm">
            Pet asset unavailable
          </div>
        ) : (
          <img
            draggable={false}
            src={imageSrc}
            alt={pet.name}
            className="sprite-pet-image"
            onError={() => {
              if (!useLegacyFrame) {
                setUseLegacyFrame(true);
                return;
              }
              setLoadFailed(true);
            }}
            onLoad={() => {
              setUseLegacyFrame(false);
              setLoadFailed(false);
            }}
          />
        )}
      </div>
    </div>
  );
}

export function SpritePetOverlay() {
  const { pets, setPets } = usePetSettings();
  const enabledPets = useMemo(
    () => pets.filter((pet) => pet.enabled && pet.assetBasePath.trim().length > 0),
    [pets]
  );

  const updatePetOffset = (id: string, x: number, y: number) => {
    setPets((current) =>
      current.map((pet) => (pet.id === id ? { ...pet, x: Math.round(x), y: Math.round(y) } : pet))
    );
  };

  if (enabledPets.length === 0) return null;

  return (
    <div className="pointer-events-none fixed inset-0 z-40">
      {enabledPets.map((pet) => (
        <div key={pet.id} className="pointer-events-auto">
          <SpritePet pet={pet} updatePetOffset={updatePetOffset} />
        </div>
      ))}
    </div>
  );
}
