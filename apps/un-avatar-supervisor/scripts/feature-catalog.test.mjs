import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";
const source = await readFile(new URL("../src/lib/avatarFeatureCatalog.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } });
const { avatarFeatures, findAvatarFeatures } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
test("purpose, technical and full-width terms reach the existing feature", () => {
	assert.deepEqual(
		findAvatarFeatures("Ｓｐｏｕｔ２").map((f) => f.id),
		["output"]
	);
	assert.deepEqual(
		findAvatarFeatures("揺れ").map((f) => f.id),
		["physics"]
	);
	assert.deepEqual(
		findAvatarFeatures("複数購読").map((f) => f.id),
		["motion"]
	);
	assert.deepEqual(
		findAvatarFeatures("texture compression").map((f) => f.id),
		["quality"]
	);
	assert.equal(findAvatarFeatures("no-such-feature").length, 0);
});
test("all existing profile sections are reachable and focusable", async () => {
	const types = await readFile(new URL("../src/lib/profileTypes.ts", import.meta.url), "utf8");
	const union = types.match(/export type ProfileSectionId =([\s\S]*?);/)[1];
	const ids = [...union.matchAll(/"(.*?)"/g)].map((m) => m[1]);
	assert.deepEqual(
		avatarFeatures
			.filter((f) => f.section)
			.map((f) => f.section)
			.sort(),
		ids.sort()
	);
	for (const id of ids) {
		const name = id[0].toUpperCase() + id.slice(1);
		const component = await readFile(new URL(`../src/lib/Profile${name}Section.svelte`, import.meta.url), "utf8");
		assert.match(component, new RegExp(`<section[^>]*tabindex="-1"[^>]*data-profile-section="${id}"`));
	}
});
