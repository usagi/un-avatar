export const WINDOW_SIZE_FIELDS = [
	{
		key: "width",
		labelKey: "profiles.editor.width_px",
		field: "window.width",
		min: 160,
		max: 8192,
		step: 1,
	},
	{
		key: "height",
		labelKey: "profiles.editor.height_px",
		field: "window.height",
		min: 160,
		max: 8192,
		step: 1,
	},
] as const;

export const WINDOW_POSITION_FIELDS = [
	{
		key: "x",
		labelKey: "profiles.editor.x_px",
		hintKey: "profiles.hints.window.position_x",
		field: "window.x",
	},
	{
		key: "y",
		labelKey: "profiles.editor.y_px",
		hintKey: "profiles.hints.window.position_y",
		field: "window.y",
	},
] as const;

export const WINDOW_POSITION_MIN = -32768;
export const WINDOW_POSITION_MAX = 32767;

export const WINDOW_BACKGROUND_FALLBACK: [number, number, number] = [0.12, 0.14, 0.18];
