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
	preview_images: UnavatarPreviewImage[];
};

export type UnavatarPreviewImage = {
	view: string | null;
	data_url: string;
};

export type UnavatarMetadataDialogState = {
	metadata: UnavatarMetadataInfo;
	pendingPath: string | null;
};

export function looksLikeUnavatarPath(path: string): boolean {
	return path.trim().toLowerCase().endsWith(".unavatar");
}
