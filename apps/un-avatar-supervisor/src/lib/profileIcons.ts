export const DEFAULT_PROFILE_ICON_SRC = "/un-avatar-artwork-supervisor.png";

type FileSrcConverter = (path: string) => string;

export function isThumbnailCachePath(path: string): boolean {
	const normalized = path.replaceAll("\\", "/").toLowerCase();
	return normalized.endsWith(".webp") && normalized.includes("/profiles/assets/thumbnails/");
}

export function thumbnailFileUrl(path: string): string {
	const fileName = path.replaceAll("\\", "/").split("/").pop();
	if (!fileName) return DEFAULT_PROFILE_ICON_SRC;
	return `http://un-avatar-thumbnail.localhost/${encodeURIComponent(fileName)}`;
}

export function profileIconSrc(path: string | null, hasRuntime: boolean, convertFileSrc: FileSrcConverter): string {
	if (!path) return DEFAULT_PROFILE_ICON_SRC;
	if (path.startsWith("data:image/") || path.startsWith("http://") || path.startsWith("https://") || path.startsWith("/")) {
		return path;
	}
	if (path.endsWith("un-avatar-design-master.png") || path.endsWith("un-avatar-artwork-supervisor.png")) {
		return DEFAULT_PROFILE_ICON_SRC;
	}
	if (!hasRuntime) return DEFAULT_PROFILE_ICON_SRC;
	return isThumbnailCachePath(path) ? thumbnailFileUrl(path) : convertFileSrc(path);
}
