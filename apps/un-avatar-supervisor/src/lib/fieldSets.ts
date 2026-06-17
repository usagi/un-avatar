export function fieldSetIncludes(fields: readonly string[], target: string): boolean {
	return fields.includes(target);
}

export function fieldSetIncludesAny(fields: readonly string[], targets: readonly string[]): boolean {
	for (const target of targets) {
		if (fields.includes(target)) return true;
	}
	return false;
}

export function fieldSetHas(fields: readonly string[], predicate: (field: string) => boolean): boolean {
	return fields.some(predicate);
}

export function fieldSetStartsWith(fields: readonly string[], prefix: string): boolean {
	for (const field of fields) {
		if (field.startsWith(prefix)) return true;
	}
	return false;
}
