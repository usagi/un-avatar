<script lang="ts">
	export let label: string;
	export let phaseTag: string | null = null;
	export let hint: string | undefined = undefined;
	export let checked: boolean;
	export let disabled = false;
	export let onChange: (checked: boolean) => void | Promise<void>;
</script>

<label class="toggle-field" data-hint={hint}
	><input type="checkbox" {checked} {disabled} onchange={(event) => onChange((event.currentTarget as HTMLInputElement).checked)} /><span
		>{label}{#if phaseTag}
			<span class="phase-tag">{phaseTag}</span>
		{/if}</span
	></label
>
