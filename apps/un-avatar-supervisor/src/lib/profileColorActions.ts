import type { ColorDisplayMode } from "./storageState";

export type ColorModeChangeHandler = (mode: ColorDisplayMode) => void | Promise<void>;
