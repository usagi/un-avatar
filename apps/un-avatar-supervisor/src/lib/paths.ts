export function normalizedPathKey(path: string | null | undefined): string {
	return (path ?? "").replaceAll("\\", "/").toLowerCase();
}

export function sameNormalizedPath(leftPath: string | null | undefined, rightPath: string | null | undefined): boolean {
	const left = normalizedPathKey(leftPath);
	const right = normalizedPathKey(rightPath);
	return left.length > 0 && left === right;
}
