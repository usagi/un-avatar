export type CameraOrbitPreset = "left" | "front" | "right";

export type CameraOrbitPresetAngles = {
  longitude: number;
  latitude: number;
};

const CAMERA_ORBIT_PRESETS: Record<CameraOrbitPreset, CameraOrbitPresetAngles> = {
  left: { longitude: 145, latitude: 0 },
  front: { longitude: 180, latitude: 0 },
  right: { longitude: 215, latitude: 0 },
};

export function cameraOrbitPresetAngles(kind: CameraOrbitPreset): CameraOrbitPresetAngles {
  return CAMERA_ORBIT_PRESETS[kind];
}
