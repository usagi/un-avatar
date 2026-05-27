export type CameraDiagramSource = {
  camera_target: Vec3 | null;
  camera_longitude_deg: number | null;
  camera_latitude_deg: number | null;
  camera_radius: number | null;
  camera_diagonal_fov_deg: number | null;
  window_width?: number | null;
  window_height?: number | null;
};

export type LightingDiagramSource = CameraDiagramSource & {
  lighting_directional_follow_camera_yaw: boolean;
  lighting_directional_follow_camera_pitch: boolean;
  lighting_directional_azimuth_deg: number;
  lighting_directional_elevation_deg: number;
};

export type CameraDiagram = {
  gridOffsets: readonly number[];
  originX: number;
  originY: number;
  sideOriginX: number;
  sideOriginY: number;
  topScale: number;
  sideScale: number;
  cameraX: number;
  cameraY: number;
  targetX: number;
  targetY: number;
  fovLeftX: number;
  fovLeftY: number;
  fovRightX: number;
  fovRightY: number;
  sideCameraX: number;
  sideCameraY: number;
  sideTargetX: number;
  sideTargetY: number;
  sideFovLeftX: number;
  sideFovLeftY: number;
  sideFovRightX: number;
  sideFovRightY: number;
  topOrbitRx: number;
  topOrbitRy: number;
  sideOrbitRx: number;
  sideOrbitRy: number;
  radiusLabel: string;
  fovLabel: string;
};

export type LightingDiagram = {
  gridOffsets: readonly number[];
  originX: number;
  originY: number;
  sideOriginX: number;
  sideOriginY: number;
  scale: number;
  cameraX: number;
  cameraY: number;
  targetX: number;
  targetY: number;
  rayStartX: number;
  rayStartY: number;
  rayEndX: number;
  rayEndY: number;
  basisEndX: number;
  basisEndY: number;
  sideCameraX: number;
  sideCameraY: number;
  sideTargetX: number;
  sideTargetY: number;
  sideRayStartX: number;
  sideRayStartY: number;
  sideRayEndX: number;
  sideRayEndY: number;
  sideProjectionX: number;
  sideProjectionY: number;
  basisLabel: string;
  azimuthLabel: string;
  elevationLabel: string;
};

type Vec3 = [number, number, number];
type Point2 = readonly [number, number];

const VIEW_CENTER_X = 110;
const VIEW_CENTER_Y = 68;
const GRID_OFFSETS = [-4, -3, -2, -1, 1, 2, 3, 4] as const;

function degreesToRadians(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

function radiansToDegrees(radians: number): number {
  return (radians * 180) / Math.PI;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function normalize3(vector: Vec3): Vec3 {
  const length = Math.hypot(vector[0], vector[1], vector[2]) || 1;
  return [vector[0] / length, vector[1] / length, vector[2] / length];
}

function normalizeHorizontal(vector: Vec3): Vec3 {
  const length = Math.hypot(vector[0], vector[2]) || 1;
  return [vector[0] / length, 0, vector[2] / length];
}

export function diagonalFovFromLensMm(focalLengthMm: number): number {
  const fullFrameDiagonalMm = Math.sqrt(36 * 36 + 24 * 24);
  return radiansToDegrees(2 * Math.atan(fullFrameDiagonalMm / (2 * focalLengthMm)));
}

export function verticalFovToDiagonal(verticalDeg: number, width: number, height: number): number {
  const aspect = Math.max(1, width) / Math.max(1, height);
  const verticalRad = degreesToRadians(verticalDeg);
  const diagonalRad = 2 * Math.atan(Math.tan(verticalRad / 2) * Math.sqrt(1 + aspect * aspect));
  return radiansToDegrees(diagonalRad);
}

export function horizontalFovToDiagonal(horizontalDeg: number, width: number, height: number): number {
  const aspect = Math.max(1, width) / Math.max(1, height);
  const horizontalRad = degreesToRadians(horizontalDeg);
  const verticalRad = 2 * Math.atan(Math.tan(horizontalRad / 2) / aspect);
  return verticalFovToDiagonal(radiansToDegrees(verticalRad), width, height);
}

export function diagonalFovToVertical(diagonalDeg: number, width: number, height: number): number {
  const aspect = Math.max(1, width) / Math.max(1, height);
  const diagonalRad = degreesToRadians(diagonalDeg);
  const verticalRad = 2 * Math.atan(Math.tan(diagonalRad / 2) / Math.sqrt(1 + aspect * aspect));
  return radiansToDegrees(verticalRad);
}

export function diagonalFovToHorizontal(diagonalDeg: number, width: number, height: number): number {
  const aspect = Math.max(1, width) / Math.max(1, height);
  const verticalDeg = diagonalFovToVertical(diagonalDeg, width, height);
  const verticalRad = degreesToRadians(verticalDeg);
  const horizontalRad = 2 * Math.atan(Math.tan(verticalRad / 2) * aspect);
  return radiansToDegrees(horizontalRad);
}

export function lensMmFromDiagonalFov(diagonalDeg: number): number {
  const fullFrameDiagonalMm = Math.sqrt(36 * 36 + 24 * 24);
  const diagonalRad = degreesToRadians(diagonalDeg);
  return fullFrameDiagonalMm / (2 * Math.tan(diagonalRad / 2));
}

export function cameraDiagram(setting: CameraDiagramSource): CameraDiagram {
  const lon = degreesToRadians(setting.camera_longitude_deg ?? 180);
  const lat = degreesToRadians(setting.camera_latitude_deg ?? 0);
  const radiusM = Math.max(0.05, setting.camera_radius ?? 1.5);
  const target = setting.camera_target ?? [0, 0, 0];
  const [targetWorldX, targetWorldY, targetWorldZ] = target;
  const width = setting.window_width ?? 1280;
  const height = setting.window_height ?? 720;
  const horizontalFovRad = degreesToRadians(diagonalFovToHorizontal(setting.camera_diagonal_fov_deg ?? 35, width, height));
  const verticalFovRad = degreesToRadians(diagonalFovToVertical(setting.camera_diagonal_fov_deg ?? 35, width, height));
  const scale = Math.min(
    38,
    96 / Math.max(Math.abs(targetWorldX) + radiusM + 0.35, 2.5),
    56 / Math.max(Math.abs(targetWorldZ) + radiusM + 0.35, 1.5),
    96 / Math.max(Math.abs(targetWorldZ) + radiusM + 0.35, 2.5),
    56 / Math.max(Math.abs(targetWorldY) + radiusM + 0.35, 1.5),
  );
  const topScale = scale;
  const sideScale = scale;
  const topOrbitRx = radiusM * topScale;
  const topOrbitRy = radiusM * topScale;
  const sideOrbitRx = radiusM * sideScale;
  const sideOrbitRy = radiusM * sideScale;
  const cameraWorldX = Math.sin(lon) * Math.cos(lat);
  const cameraWorldY = Math.sin(lat);
  const cameraWorldZ = -Math.cos(lon) * Math.cos(lat);
  const originX = VIEW_CENTER_X - targetWorldX * topScale;
  const originY = VIEW_CENTER_Y - targetWorldZ * topScale;
  const targetX = VIEW_CENTER_X;
  const targetY = VIEW_CENTER_Y;
  const cameraX = targetX + cameraWorldX * topScale * radiusM;
  const cameraY = targetY + cameraWorldZ * topScale * radiusM;
  const angle = Math.atan2(targetY - cameraY, targetX - cameraX);
  const minHalfFov = degreesToRadians(2);
  const maxHalfFov = degreesToRadians(80);
  const topHalfFov = Math.min(maxHalfFov, Math.max(minHalfFov, horizontalFovRad / 2));
  const sideHalfFov = Math.min(maxHalfFov, Math.max(minHalfFov, verticalFovRad / 2));
  const topFovLen = Math.min(72, Math.max(28, radiusM * topScale * 1.15));
  const sideFovLen = Math.min(72, Math.max(28, radiusM * sideScale * 1.15));
  const sideOriginX = VIEW_CENTER_X + targetWorldZ * sideScale;
  const sideOriginY = VIEW_CENTER_Y + targetWorldY * sideScale;
  const sideTargetX = VIEW_CENTER_X;
  const sideTargetY = VIEW_CENTER_Y;
  const sideCameraX = sideTargetX - cameraWorldZ * sideScale * radiusM;
  const sideCameraY = sideTargetY - cameraWorldY * sideScale * radiusM;
  const sideAngle = Math.atan2(sideTargetY - sideCameraY, sideTargetX - sideCameraX);
  return {
    gridOffsets: GRID_OFFSETS,
    originX,
    originY,
    sideOriginX,
    sideOriginY,
    topScale,
    sideScale,
    cameraX,
    cameraY,
    targetX,
    targetY,
    fovLeftX: cameraX + Math.cos(angle - topHalfFov) * topFovLen,
    fovLeftY: cameraY + Math.sin(angle - topHalfFov) * topFovLen,
    fovRightX: cameraX + Math.cos(angle + topHalfFov) * topFovLen,
    fovRightY: cameraY + Math.sin(angle + topHalfFov) * topFovLen,
    sideCameraX,
    sideCameraY,
    sideTargetX,
    sideTargetY,
    sideFovLeftX: sideCameraX + Math.cos(sideAngle - sideHalfFov) * sideFovLen,
    sideFovLeftY: sideCameraY + Math.sin(sideAngle - sideHalfFov) * sideFovLen,
    sideFovRightX: sideCameraX + Math.cos(sideAngle + sideHalfFov) * sideFovLen,
    sideFovRightY: sideCameraY + Math.sin(sideAngle + sideHalfFov) * sideFovLen,
    topOrbitRx,
    topOrbitRy,
    sideOrbitRx,
    sideOrbitRy,
    radiusLabel: `${formatFixed((setting.camera_radius ?? 1.5) * 1000, 0)} mm`,
    fovLabel: `${formatFixed(setting.camera_diagonal_fov_deg ?? 35)} deg`,
  };
}

export function lightingDiagram(setting: LightingDiagramSource): LightingDiagram {
  const lon = degreesToRadians(setting.camera_longitude_deg ?? 180);
  const lat = degreesToRadians(setting.camera_latitude_deg ?? 0);
  const radiusM = Math.max(0.05, setting.camera_radius ?? 1.5);
  const target = setting.camera_target ?? [0, 0, 0];
  const followCameraYaw = setting.lighting_directional_follow_camera_yaw;
  const cameraOffset: Vec3 = [
    Math.sin(lon) * Math.cos(lat) * radiusM,
    Math.sin(lat) * radiusM,
    -Math.cos(lon) * Math.cos(lat) * radiusM,
  ];
  const cameraPos: Vec3 = [
    target[0] + cameraOffset[0],
    target[1] + cameraOffset[1],
    target[2] + cameraOffset[2],
  ];
  const cameraDir = normalize3([
    cameraPos[0] - target[0],
    cameraPos[1] - target[1],
    cameraPos[2] - target[2],
  ]);
  const cameraYaw = normalizeHorizontal(cameraDir);
  const baseDir: Vec3 = followCameraYaw ? cameraYaw : [0, 0, 1];
  const lightRight = normalizeHorizontal([baseDir[2], 0, -baseDir[0]]);
  const lightAzimuth = degreesToRadians(setting.lighting_directional_azimuth_deg);
  const cameraPitchDeg = setting.lighting_directional_follow_camera_pitch
    ? radiansToDegrees(Math.asin(clamp(cameraDir[1], -1, 1)))
    : 0;
  const lightElevation = degreesToRadians(
    clamp(setting.lighting_directional_elevation_deg + cameraPitchDeg, -89, 89),
  );
  const localX = Math.sin(lightAzimuth) * Math.cos(lightElevation);
  const localY = Math.sin(lightElevation);
  const localZ = Math.cos(lightElevation);
  const lightDirRaw: Vec3 = [
    lightRight[0] * localX + baseDir[0] * localZ,
    localY + baseDir[1] * localZ,
    lightRight[2] * localX + baseDir[2] * localZ,
  ];
  const lightDir = normalize3(lightDirRaw);
  const rayLengthM = clamp(radiusM * 0.8, 1.2, 3.0);
  const rayEndM = target;
  const basisEndM: Vec3 = [
    rayEndM[0] + baseDir[0] * rayLengthM,
    rayEndM[1],
    rayEndM[2] + baseDir[2] * rayLengthM,
  ];
  const rayStartM: Vec3 = [
    rayEndM[0] + lightDir[0] * rayLengthM,
    rayEndM[1] + lightDir[1] * rayLengthM,
    rayEndM[2] + lightDir[2] * rayLengthM,
  ];
  const sideProjectionM: Vec3 = [rayStartM[0], rayEndM[1], rayStartM[2]];
  const points = [target, cameraPos, rayStartM, rayEndM, basisEndM, sideProjectionM];
  let topExtentX = Math.abs(target[0]);
  let topExtentZ = Math.abs(target[2]);
  let sideExtentZ = Math.abs(target[2]);
  let sideExtentY = Math.abs(target[1]);
  for (const point of points) {
    topExtentX = Math.max(topExtentX, Math.abs(point[0] - target[0]));
    topExtentZ = Math.max(topExtentZ, Math.abs(point[2] - target[2]));
    sideExtentZ = Math.max(sideExtentZ, Math.abs(point[2] - target[2]));
    sideExtentY = Math.max(sideExtentY, Math.abs(point[1] - target[1]));
  }
  const scale = Math.min(
    38,
    96 / Math.max(topExtentX + 0.35, 2.5),
    56 / Math.max(topExtentZ + 0.35, 1.5),
    96 / Math.max(sideExtentZ + 0.35, 2.5),
    56 / Math.max(sideExtentY + 0.35, 1.5),
  );
  const originX = VIEW_CENTER_X - target[0] * scale;
  const originY = VIEW_CENTER_Y - target[2] * scale;
  const sideOriginX = VIEW_CENTER_X + target[2] * scale;
  const sideOriginY = VIEW_CENTER_Y + target[1] * scale;
  const top = (p: Vec3): Point2 => [originX + p[0] * scale, originY + p[2] * scale] as const;
  const side = (p: Vec3): Point2 => [sideOriginX - p[2] * scale, sideOriginY - p[1] * scale] as const;
  const [cameraX, cameraY] = top(cameraPos);
  const [targetX, targetY] = top(target);
  const [rayStartX, rayStartY] = top(rayStartM);
  const [rayEndX, rayEndY] = top(rayEndM);
  const [basisEndX, basisEndY] = top(basisEndM);
  const [sideCameraX, sideCameraY] = side(cameraPos);
  const [sideTargetX, sideTargetY] = side(target);
  const [sideRayStartX, sideRayStartY] = side(rayStartM);
  const [sideRayEndX, sideRayEndY] = side(rayEndM);
  const [sideProjectionX, sideProjectionY] = side(sideProjectionM);
  return {
    gridOffsets: GRID_OFFSETS,
    originX,
    originY,
    sideOriginX,
    sideOriginY,
    scale,
    cameraX,
    cameraY,
    targetX,
    targetY,
    rayStartX,
    rayStartY,
    rayEndX,
    rayEndY,
    basisEndX,
    basisEndY,
    sideCameraX,
    sideCameraY,
    sideTargetX,
    sideTargetY,
    sideRayStartX,
    sideRayStartY,
    sideRayEndX,
    sideRayEndY,
    sideProjectionX,
    sideProjectionY,
    basisLabel: followCameraYaw ? "Camera" : "World",
    azimuthLabel: `${formatFixed(setting.lighting_directional_azimuth_deg, 0)} deg`,
    elevationLabel: `${formatFixed(setting.lighting_directional_elevation_deg + cameraPitchDeg, 0)} deg`,
  };
}

function formatFixed(value: number | null | undefined, digits = 2): string {
  if (value == null || !Number.isFinite(value)) return "";
  return value.toFixed(digits);
}
