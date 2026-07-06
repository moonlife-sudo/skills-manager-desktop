import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { cn } from "../utils";
import {
  usePetSettings,
  type PetAction,
  type PetPosition,
  type SpritePetConfig,
} from "../hooks/usePetSettings";

function positionClass(position: PetPosition) {
  switch (position) {
    case "sidebar":
      return "left-5 bottom-8";
    case "bottom-left":
      return "left-7 bottom-8";
    case "top-right":
      return "right-7 top-16";
    default:
      return "right-7 bottom-8";
  }
}

function normalizeBasePath(path: string) {
  return path.trim().replace(/[\\/]+$/, "");
}

function frameSrc(basePath: string, action: PetAction) {
  const base = normalizeBasePath(basePath || "/pets/academy-assistant");
  const relative = `${base}/${action}.png`;
  if (/^(https?:|data:|blob:|\/)/i.test(relative)) return relative;
  return convertFileSrc(relative.replace(/\//g, "\\"));
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

  const displayedAction = temporaryAction ?? pet.idleAction;
  const displayedOffset = temporaryOffset ?? { x: pet.x, y: pet.y };
  const imageSrc = useMemo(() => frameSrc(pet.assetBasePath, displayedAction), [displayedAction, pet.assetBasePath]);

  useEffect(() => () => {
    if (timerRef.current != null) window.clearTimeout(timerRef.current);
  }, []);

  const triggerAction = (nextAction: PetAction, duration = 1400) => {
    if (timerRef.current != null) window.clearTimeout(timerRef.current);
    setTemporaryAction(nextAction);
    timerRef.current = window.setTimeout(() => {
      setTemporaryAction(null);
      timerRef.current = null;
    }, duration);
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!pet.draggable) return;
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
    setTemporaryOffset({
      x: Math.max(-360, Math.min(360, drag.startX + dx)),
      y: Math.max(-360, Math.min(360, drag.startY + dy)),
    });
  };

  const finishPointer = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const finalOffset = {
      x: Math.max(-360, Math.min(360, drag.startX + event.clientX - drag.startClientX)),
      y: Math.max(-360, Math.min(360, drag.startY + event.clientY - drag.startClientY)),
    };
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
        "fixed z-40 select-none",
        pet.draggable ? "cursor-grab active:cursor-grabbing" : "cursor-pointer",
        positionClass(pet.position)
      )}
      style={{
        transform: `translate(${displayedOffset.x}px, ${displayedOffset.y}px) scale(${pet.scale})`,
        transformOrigin:
          pet.position === "bottom-left" || pet.position === "sidebar"
            ? "bottom left"
            : pet.position === "top-right"
              ? "top right"
              : "bottom right",
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
            onError={() => setLoadFailed(true)}
            onLoad={() => setLoadFailed(false)}
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
