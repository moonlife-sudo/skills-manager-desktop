import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/tauri";

export type PetPosition = "sidebar" | "bottom-left" | "bottom-right" | "top-right";

export interface Live2DPetConfig {
  id: string;
  name: string;
  enabled: boolean;
  modelPath: string;
  position: PetPosition;
  scale: number;
  opacity: number;
  x: number;
  y: number;
}

const STORAGE_KEY = "appearance.live2dPets";
const SETTINGS_KEY = "appearance_live2d_pets";
const EVENT_NAME = "skills-manager-pets-changed";

const DEFAULT_PETS: Live2DPetConfig[] = [
  {
    id: "bundled-hiyori",
    name: "Hiyori",
    enabled: true,
    modelPath: "/live2d/Hiyori/Hiyori.model3.json",
    position: "bottom-right",
    scale: 0.72,
    opacity: 1,
    x: 0,
    y: 0,
  },
  {
    id: "bundled-mao",
    name: "Mao",
    enabled: false,
    modelPath: "/live2d/Mao/Mao.model3.json",
    position: "sidebar",
    scale: 0.58,
    opacity: 0.95,
    x: 0,
    y: 0,
  },
  {
    id: "bundled-wanko",
    name: "Wanko",
    enabled: false,
    modelPath: "/live2d/Wanko/Wanko.model3.json",
    position: "bottom-left",
    scale: 0.64,
    opacity: 0.95,
    x: 0,
    y: 0,
  },
];

function safeParsePets(value: string | null): Live2DPetConfig[] {
  if (!value) return DEFAULT_PETS;
  try {
    const parsed = JSON.parse(value);
    if (!Array.isArray(parsed)) return DEFAULT_PETS;
    if (parsed.length === 0) return DEFAULT_PETS;
    return parsed
      .filter((item) => item && typeof item === "object")
      .map((item, index) => ({
        id: typeof item.id === "string" ? item.id : `${Date.now()}-${index}`,
        name: typeof item.name === "string" ? item.name : `Pet ${index + 1}`,
        enabled: typeof item.enabled === "boolean" ? item.enabled : true,
        modelPath: typeof item.modelPath === "string" ? item.modelPath : "",
        position: isPetPosition(item.position) ? item.position : "bottom-right",
        scale: clampNumber(item.scale, 0.35, 2, 0.8),
        opacity: clampNumber(item.opacity, 0.2, 1, 1),
        x: clampNumber(item.x, -240, 240, 0),
        y: clampNumber(item.y, -240, 240, 0),
      }));
  } catch {
    return DEFAULT_PETS;
  }
}

function isPetPosition(value: unknown): value is PetPosition {
  return value === "sidebar" || value === "bottom-left" || value === "bottom-right" || value === "top-right";
}

function clampNumber(value: unknown, min: number, max: number, fallback: number) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return fallback;
  return Math.min(max, Math.max(min, numeric));
}

function readLocalPets() {
  return safeParsePets(localStorage.getItem(STORAGE_KEY));
}

function persistPets(pets: Live2DPetConfig[]) {
  const payload = JSON.stringify(pets);
  localStorage.setItem(STORAGE_KEY, payload);
  void api.setSettings(SETTINGS_KEY, payload);
  window.dispatchEvent(new CustomEvent(EVENT_NAME, { detail: pets }));
}

export function usePetSettings() {
  const [pets, setPetsState] = useState<Live2DPetConfig[]>(() => readLocalPets());

  useEffect(() => {
    let cancelled = false;
    api.getSettings(SETTINGS_KEY).then((value) => {
      if (cancelled || value == null) return;
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
      const custom = event as CustomEvent<Live2DPetConfig[]>;
      setPetsState(Array.isArray(custom.detail) ? custom.detail : readLocalPets());
    };
    window.addEventListener(EVENT_NAME, handler);
    window.addEventListener("storage", handler);
    return () => {
      window.removeEventListener(EVENT_NAME, handler);
      window.removeEventListener("storage", handler);
    };
  }, []);

  const setPets = useCallback((next: Live2DPetConfig[] | ((current: Live2DPetConfig[]) => Live2DPetConfig[])) => {
    setPetsState((current) => {
      const resolved = typeof next === "function" ? next(current) : next;
      persistPets(resolved);
      return resolved;
    });
  }, []);

  return { pets, setPets };
}

export function createDefaultPet(): Live2DPetConfig {
  const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return {
    id,
    name: "Live2D Pet",
    enabled: true,
    modelPath: "",
    position: "bottom-right",
    scale: 0.8,
    opacity: 1,
    x: 0,
    y: 0,
  };
}
