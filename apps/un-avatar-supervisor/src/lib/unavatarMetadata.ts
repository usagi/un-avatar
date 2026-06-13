import { basename } from "./formatting";

export type UnavatarMetadataInfo = {
	path: string;
	file_name: string;
	name: string | null;
	spec_version: string | null;
	generator: string | null;
	source_type: string | null;
	export_mode: string | null;
	created_utc: string | null;
	wardrobe_set_count: number;
	dynamics_count: number;
	contact_count: number;
	modular_avatar_component_count: number;
	redistribution_allowed: boolean | null;
	sample_screenshot_data_url: string | null;
};

export type UnavatarMetadataDialogState = {
	metadata: UnavatarMetadataInfo;
	pendingPath: string | null;
};

export function looksLikeUnavatarPath(path: string): boolean {
	return path.trim().toLowerCase().endsWith(".unavatar");
}

export function fallbackUnavatarMetadata(path: string): UnavatarMetadataInfo {
	return {
		path,
		file_name: basename(path),
		name: basename(path),
		spec_version: null,
		generator: null,
		source_type: null,
		export_mode: null,
		created_utc: null,
		wardrobe_set_count: 0,
		dynamics_count: 0,
		contact_count: 0,
		modular_avatar_component_count: 0,
		redistribution_allowed: null,
		sample_screenshot_data_url: null,
	};
}
