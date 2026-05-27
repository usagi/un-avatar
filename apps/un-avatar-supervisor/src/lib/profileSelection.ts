export type ProfileIdentity = {
  id: string;
};

export type SortableProfileIdentity = ProfileIdentity & {
  sort_order: number;
  created_at: string;
  name: string;
};

export type ProfileLaunchTargetIdentity = ProfileIdentity & {
  group: string;
};

export function pickInitialSelectedSettingId<T extends ProfileIdentity>(
  current: string | null,
  remembered: string | null,
  settings: readonly T[],
): string | null {
  if (current && settings.some((setting) => setting.id === current)) {
    return current;
  }
  if (remembered && settings.some((setting) => setting.id === remembered)) {
    return remembered;
  }
  return settings[0]?.id ?? null;
}

export function compareAvatarSettings<T extends SortableProfileIdentity>(
  a: T,
  b: T,
): number {
  return (
    a.sort_order - b.sort_order ||
    a.created_at.localeCompare(b.created_at) ||
    a.name.localeCompare(b.name) ||
    a.id.localeCompare(b.id)
  );
}

export function isValidLaunchTarget<T extends ProfileLaunchTargetIdentity>(
  value: string | null,
  settings: readonly T[],
): boolean {
  if (!value) return false;
  if (value.startsWith("group:")) {
    const group = value.slice("group:".length).trim();
    return group.length > 0 && settings.some((setting) => setting.group.trim() === group);
  }
  return settings.some((setting) => setting.id === value);
}

export function pickInitialLaunchTargetId<T extends ProfileLaunchTargetIdentity>(
  current: string | null,
  selected: string | null,
  settings: readonly T[],
): string | null {
  if (isValidLaunchTarget(current, settings)) return current;
  if (isValidLaunchTarget(selected, settings)) return selected;
  return settings[0]?.id ?? null;
}
