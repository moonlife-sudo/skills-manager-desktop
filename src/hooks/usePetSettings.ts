import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/tauri";

export type PetAction =
  | "idle"
  | "blink"
  | "wave"
  | "happy"
  | "thinking"
  | "typing"
  | "surprised"
  | "drag"
  | "sleepy"
  | "sleep"
  | "sad"
  | "celebrate";

export interface SpritePetConfig {
  id: string;
  name: string;
  enabled: boolean;
  assetBasePath: string;
  scale: number;
  opacity: number;
  x: number;
  y: number;
  idleAction: PetAction;
  clickAction: PetAction;
  animationFps: number;
  randomActions: boolean;
  randomActionIntervalSec: number;
}

const STORAGE_KEY = "appearance.spritePets";
const SETTINGS_KEY = "appearance_sprite_pets";
const EVENT_NAME = "skills-manager-pets-changed";
const DEFAULT_ASSET_BASE_PATH = "/pets/academy-assistant";

const PET_ACTIONS: PetAction[] = [
  "idle",
  "blink",
  "wave",
  "happy",
  "thinking",
  "typing",
  "surprised",
  "drag",
  "sleepy",
  "sleep",
  "sad",
  "celebrate",
];

const DEFAULT_PETS: SpritePetConfig[] = [
  {
    id: "academy-assistant",
    name: "Academy Assistant",
    enabled: true,
    assetBasePath: DEFAULT_ASSET_BASE_PATH,
    scale: 0.82,
    opacity: 1,
    x: defaultPetCoordinates().x,
    y: defaultPetCoordinates().y,
    idleAction: "idle",
    clickAction: "happy",
    animationFps: 8,
    randomActions: true,
    randomActionIntervalSec: 12,
  },
];

function safeParsePets(value: string | null): SpritePetConfig[] {
  if (!value) return DEFAULT_PETS;
  try {
    const parsed = JSON.parse(value);
    if (!Array.isArray(parsed)) return DEFAULT_PETS;
    if (parsed.length === 0) return DEFAULT_PETS;
    return parsed
      .filter((item) => item && typeof item === "object")
      .map((item, index) => {
        const fallback = defaultPetCoordinates(isLegacyPosition(item.position) ? item.position : undefined);
        const legacyAnchored = isLegacyPosition(item.position);
        return {
          id: typeof item.id === "string" ? item.id : `${Date.now()}-${index}`,
          name: typeof item.name === "string" ? item.name : `Pet ${index + 1}`,
          enabled: typeof item.enabled === "boolean" ? item.enabled : index === 0,
          assetBasePath:
            typeof item.assetBasePath === "string" && item.assetBasePath.trim()
              ? item.assetBasePath.trim()
              : DEFAULT_ASSET_BASE_PATH,
          scale: clampNumber(item.scale, 0.35, 2, 0.82),
          opacity: clampNumber(item.opacity, 0.2, 1, 1),
          x: clampNumber(item.x, -2000, 4000, 0) + (legacyAnchored ? fallback.x : 0),
          y: clampNumber(item.y, -2000, 4000, 0) + (legacyAnchored ? fallback.y : 0),
          idleAction: isPetAction(item.idleAction) ? item.idleAction : "idle",
          clickAction: isPetAction(item.clickAction) ? item.clickAction : "happy",
          animationFps: clampNumber(item.animationFps, 2, 24, 8),
          randomActions: typeof item.randomActions === "boolean" ? item.randomActions : true,
          randomActionIntervalSec: clampNumber(item.randomActionIntervalSec, 4, 120, 12),
        };
      });
  } catch {
    return DEFAULT_PETS;
  }
}

function isLegacyPosition(value: unknown): value is "sidebar" | "bottom-left" | "bottom-right" | "top-right" {
  return value === "sidebar" || value === "bottom-left" || value === "bottom-right" || value === "top-right";
}

function isPetAction(value: unknown): value is PetAction {
  return typeof value === "string" && PET_ACTIONS.includes(value as PetAction);
}

function clampNumber(value: unknown, min: number, max: number, fallback: number) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return fallback;
  return Math.min(max, Math.max(min, numeric));
}

function defaultPetCoordinates(position: "sidebar" | "bottom-left" | "bottom-right" | "top-right" = "bottom-right") {
  const width = typeof window === "undefined" ? 1280 : window.innerWidth;
  const height = typeof window === "undefined" ? 720 : window.innerHeight;
  if (position === "sidebar" || position === "bottom-left") {
    return { x: 28, y: Math.max(96, height - 248) };
  }
  if (position === "top-right") {
    return { x: Math.max(320, width - 230), y: 76 };
  }
  return { x: Math.max(320, width - 230), y: Math.max(96, height - 248) };
}

function readLocalPets() {
  return safeParsePets(localStorage.getItem(STORAGE_KEY));
}

function persistPets(pets: SpritePetConfig[]) {
  const payload = JSON.stringify(pets);
  localStorage.setItem(STORAGE_KEY, payload);
  void api.setSettings(SETTINGS_KEY, payload);
  window.dispatchEvent(new CustomEvent(EVENT_NAME, { detail: pets }));
}

export function usePetSettings() {
  const [pets, setPetsState] = useState<SpritePetConfig[]>(() => readLocalPets());

  useEffect(() => {
    let cancelled = false;
    api.getSettings(SETTINGS_KEY).then((value) => {
      if (cancelled) return;
      const next = safeParsePets(value);
      localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
      setPetsState(next);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const handler = (event: Event) => {
      const custom = event as CustomEvent<SpritePetConfig[]>;
      setPetsState(Array.isArray(custom.detail) ? custom.detail : readLocalPets());
    };
    window.addEventListener(EVENT_NAME, handler);
    window.addEventListener("storage", handler);
    return () => {
      window.removeEventListener(EVENT_NAME, handler);
      window.removeEventListener("storage", handler);
    };
  }, []);

  const setPets = useCallback((next: SpritePetConfig[] | ((current: SpritePetConfig[]) => SpritePetConfig[])) => {
    setPetsState((current) => {
      const resolved = typeof next === "function" ? next(current) : next;
      persistPets(resolved);
      return resolved;
    });
  }, []);

  return { pets, setPets };
}

export function createDefaultPet(): SpritePetConfig {
  const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return {
    id,
    name: "Academy Assistant",
    enabled: true,
    assetBasePath: DEFAULT_ASSET_BASE_PATH,
    scale: 0.82,
    opacity: 1,
    x: defaultPetCoordinates().x,
    y: defaultPetCoordinates().y,
    idleAction: "idle",
    clickAction: "happy",
    animationFps: 8,
    randomActions: true,
    randomActionIntervalSec: 12,
  };
}

export const petActions = PET_ACTIONS;
