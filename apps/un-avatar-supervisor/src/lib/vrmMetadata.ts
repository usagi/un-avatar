import { basename } from "./formatting";

export type VrmMetadataDisplayInfo = {
	title: string | null;
	file_name: string;
	version: string | null;
	authors: string[];
};

export type VrmMetadataField = {
	label: string;
	value: string;
};

export type VrmMetadataInfo = VrmMetadataDisplayInfo & {
	path: string;
	vrm_format: string;
	spec_version: string;
	contact_information: string | null;
	references: string[];
	copyright_information: string | null;
	third_party_licenses: string | null;
	license_name: string | null;
	other_license_url: string | null;
	other_permission_url: string | null;
	thumbnail_data_url: string | null;
	technical_stats: VrmMetadataField[];
	permissions: VrmMetadataField[];
};

export type VrmMetadataDialogState = {
	metadata: VrmMetadataInfo;
	pendingPath: string | null;
};

export type VrmMetadataTranslator = (key: string, options?: { values?: Record<string, string | number> }) => string;

export function looksLikeVrmPath(path: string): boolean {
	const lower = path.trim().toLowerCase();
	return lower.endsWith(".vrm") || lower.endsWith(".glb") || lower.endsWith(".gltf");
}

export function fallbackVrmMetadata(path: string, reason: unknown, translate: VrmMetadataTranslator): VrmMetadataInfo {
	return {
		path,
		file_name: basename(path),
		vrm_format: "VRM",
		spec_version: translate("vrm_metadata.unavailable"),
		title: basename(path),
		version: null,
		authors: [],
		contact_information: null,
		references: [],
		copyright_information: null,
		third_party_licenses: `${translate("vrm_metadata.fallback_message")}\n${String(reason)}`,
		license_name: null,
		other_license_url: null,
		other_permission_url: null,
		thumbnail_data_url: null,
		technical_stats: [
			{ label: "File size", value: "unknown" },
			{ label: "Texture RAM", value: "unknown" },
		],
		permissions: [],
	};
}

const vrmMetadataFieldLabelKeys: Record<string, string> = {
	File: "file",
	Author: "author",
	Copyright: "copyright",
	Contact: "contact",
	Reference: "reference",
	License: "license",
	"Permission URL": "permission_url",
	"License URL": "license_url",
	"File size": "file_size",
	Vertices: "vertices",
	Triangles: "triangles",
	Bones: "bones",
	Textures: "textures",
	"Texture RAM": "texture_ram",
	"Morph targets": "morph_targets",
	Expressions: "expressions",
	PerfectSync: "perfect_sync",
	"Allowed user": "allowed_user",
	"Credit notation": "credit_notation",
	Redistribution: "redistribution",
	Modification: "modification",
	"Violent usage": "violent_usage",
	"Sexual usage": "sexual_usage",
	"Commercial usage": "commercial_usage",
	"Political / religious usage": "political_religious_usage",
	"Antisocial / hate usage": "antisocial_hate_usage",
};

const vrmMetadataValueKeys: Record<string, string> = {
	allow: "allow",
	true: "allow",
	required: "required",
	unnecessary: "unnecessary",
	disallow: "disallow",
	false: "disallow",
	onlyauthor: "only_author",
	onlyseparatelylicensedperson: "only_separately_licensed_person",
	everyone: "everyone",
	personalnonprofit: "personal_non_profit",
	personalprofit: "personal_profit",
	corporation: "corporation",
	redistribution_prohibited: "redistribution_prohibited",
	allowmodification: "allow_modification",
	allowmodificationredistribution: "allow_modification_redistribution",
	prohibited: "prohibited",
};

const positivePermissionValues = new Set([
	"allow",
	"true",
	"unnecessary",
	"everyone",
	"allowmodification",
	"allowmodificationredistribution",
]);
const negativePermissionValues = new Set(["disallow", "false", "prohibited", "redistribution_prohibited"]);
const limitedPermissionValues = new Set(["onlyauthor", "onlyseparatelylicensedperson"]);
const conditionalPermissionValues = new Set(["required", "personalnonprofit", "personalprofit", "corporation"]);

export function metadataTitle(metadata: VrmMetadataDisplayInfo): string {
	return metadata.title?.trim() || metadata.file_name;
}

export function metadataSubtitle(metadata: VrmMetadataDisplayInfo): string {
	const author = metadata.authors[0]?.trim();
	const version = metadata.version?.trim();
	if (author && version) return `${author} / v${version}`;
	return author || (version ? `v${version}` : "");
}

export function metadataInitial(metadata: VrmMetadataDisplayInfo): string {
	return metadataTitle(metadata).trim().slice(0, 1).toUpperCase() || "V";
}

export function metadataList(values: string[]): string {
	const parts: string[] = [];
	for (const value of values) {
		if (value.trim()) parts.push(value);
	}
	return parts.join(" / ");
}

export function vrmMetadataFieldLabel(label: string, translate: VrmMetadataTranslator): string {
	const key = vrmMetadataFieldLabelKeys[label];
	return key ? translate(`vrm_metadata.fields.${key}`) : label;
}

export function vrmMetadataFieldValue(item: VrmMetadataField, translate: VrmMetadataTranslator): string {
	if (item.value === "unknown") return translate("vrm_metadata.values.unknown");
	const normalized = item.value.trim().toLowerCase();
	const valueKey = vrmMetadataValueKeys[normalized];
	if (valueKey) return translate(`vrm_metadata.values.${valueKey}`);
	if (item.label === "Textures") {
		return item.value.replace(" · max ", ` · ${translate("vrm_metadata.values.max_texture")} `);
	}
	if (item.label !== "PerfectSync") return item.value;
	const supported = /^supported \((\d+)\/52\)$/.exec(item.value);
	if (supported) {
		return translate("vrm_metadata.values.perfect_sync_supported", {
			values: { count: supported[1] },
		});
	}
	const partial = /^partial \((\d+)\/52\)$/.exec(item.value);
	if (partial) {
		return translate("vrm_metadata.values.perfect_sync_partial", {
			values: { count: partial[1] },
		});
	}
	if (item.value === "not detected") {
		return translate("vrm_metadata.values.perfect_sync_not_detected");
	}
	return item.value;
}

export function vrmMetadataPermissionTone(item: VrmMetadataField): string {
	const normalized = item.value.trim().toLowerCase();
	if (positivePermissionValues.has(normalized)) {
		return "positive";
	}
	if (negativePermissionValues.has(normalized)) {
		return "negative";
	}
	if (limitedPermissionValues.has(normalized)) {
		return "limited";
	}
	if (conditionalPermissionValues.has(normalized)) {
		return "conditional";
	}
	return "neutral";
}
