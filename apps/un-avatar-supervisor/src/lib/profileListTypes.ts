export type ProfileListSetting = {
	id: string;
	name: string;
	manifest_path: string;
	group: string;
	icon_path: string | null;
	avatar_path: string | null;
	aa: string;
	spout_enabled: boolean;
	spout_name: string | null;
	color_look: string;
	color_look_intensity: number;
	bloom_enabled: boolean;
};

export type SettingPointerDrag = {
	active: boolean;
	currentX: number;
	currentY: number;
	offsetX: number;
	offsetY: number;
	width: number;
	height: number;
};

export type SettingDragStyle = {
	left: string | null;
	top: string | null;
	width: string | null;
	height: string | null;
};

export function settingDragStyleValues(dragging: boolean, drag: SettingPointerDrag | null): SettingDragStyle {
	if (!dragging || !drag?.active) {
		return { left: null, top: null, width: null, height: null };
	}
	return {
		left: `${drag.currentX - drag.offsetX}px`,
		top: `${drag.currentY - drag.offsetY}px`,
		width: `${drag.width}px`,
		height: `${drag.height}px`,
	};
}
