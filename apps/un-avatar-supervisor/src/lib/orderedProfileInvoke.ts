import type { invoke as tauriInvoke } from "@tauri-apps/api/core";

// These commands return or alter whole-profile snapshots. Keep their responses
// ordered within this Supervisor; renderer controls remain independent.
const profileCommands = new Set([
	"list_avatar_settings", "get_avatar_setting_details",
	"update_avatar_setting_value", "update_avatar_setting_values", "update_avatar_setting_path",
	"save_avatar_thumbnail_icon", "save_profile_icon_from_data_url",
	"duplicate_avatar_setting", "new_avatar_setting", "delete_avatar_setting",
	"restore_avatar_setting", "reorder_avatar_settings",
]);

export function orderedProfileInvoke(nativeInvoke: typeof tauriInvoke): typeof tauriInvoke {
	let tail: Promise<unknown> = Promise.resolve();
	const revisions = new Map<string, { current: string; ancestors: Set<string> }>();
	return <T>(...args: Parameters<typeof tauriInvoke>): Promise<T> => {
		if (!profileCommands.has(args[0])) return nativeInvoke<T>(...args);
		// Queuing must not allow a caller to change the submitted value meanwhile.
		const captured = structuredClone(args);
		const result = tail.then(async () => {
			const payload = captured[1] as Record<string, unknown> | undefined;
			const guarded = captured[0] === "update_avatar_setting_path" || captured[0] === "update_avatar_setting_value" || captured[0] === "update_avatar_setting_values" || captured[0] === "save_avatar_thumbnail_icon" || captured[0] === "save_profile_icon_from_data_url";
			const id = guarded && typeof payload?.settingId === "string" ? payload.settingId : undefined;
			const base = typeof payload?.expectedRevision === "string" ? payload.expectedRevision : undefined;
			const chain = id ? revisions.get(id) : undefined;
			const expected = base && chain?.ancestors.has(base) ? chain.current : base;
			if (payload && expected) payload.expectedRevision = expected;
			const saved = await nativeInvoke<T>(...captured);
			const revision = (saved as { revision?: string } | null)?.revision;
			if (id && expected && revision) {
				const ancestors = chain?.current === expected ? new Set(chain.ancestors) : new Set<string>();
				ancestors.add(expected); ancestors.add(revision);
				revisions.set(id, { current: revision, ancestors });
			}
			return saved;
		});
		// A rejected write must not prevent subsequent edits or recovery reads.
		tail = result.catch(() => undefined);
		return result;
	};
}
