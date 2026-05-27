export type ColorDisplayMode = "rgb_unorm" | "rgb_uint8" | "rgb_hex" | "hsl_unorm";

export function loadColorDisplayMode(key: string): ColorDisplayMode {
  if (typeof window === "undefined") return "rgb_uint8";
  const saved = window.localStorage.getItem(key);
  return saved === "rgb_unorm" ||
    saved === "rgb_uint8" ||
    saved === "rgb_hex" ||
    saved === "hsl_unorm"
    ? saved
    : "rgb_uint8";
}

export function saveColorDisplayMode(key: string, mode: ColorDisplayMode): void {
  if (typeof window !== "undefined") {
    window.localStorage.setItem(key, mode);
  }
}

export function loadLaunchTargetId(key: string): string | null {
  if (typeof window === "undefined") return null;
  const value = window.localStorage.getItem(key);
  return value && value.trim() ? value : null;
}

export function saveLaunchTargetId(key: string, value: string | null): void {
  if (typeof window === "undefined") return;
  if (value && value.trim()) {
    window.localStorage.setItem(key, value);
  } else {
    window.localStorage.removeItem(key);
  }
}
