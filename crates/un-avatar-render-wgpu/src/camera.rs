use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub(crate) struct OrbitCamera {
	pub(crate) target: Vec3,
	pub(crate) longitude: f32,
	pub(crate) latitude: f32,
	pub(crate) radius: f32,
	/// 対角画角（度）。35mm 換算で言う焦点距離 35mm = 約 63.45°。`GpuState::set_camera_fov` で変更可能。
	pub(crate) diagonal_fov_deg: f32,
}

const INITIAL_CAMERA_TARGET: Vec3 = Vec3::new(0.0, 1.05, 0.0);
const INITIAL_CAMERA_POS_POSITIVE_Z: Vec3 = Vec3::new(0.0, 1.55, 2.85);
/// フルサイズ換算 36×24mm に焦点距離 35mm のレンズを載せたときの対角画角（度）。
/// 35mm 相当の代表的な「自然な広角」値。
pub(crate) const DEFAULT_DIAGONAL_FOV_DEG: f32 = 63.4548;
/// 対角画角のサポート範囲。あまりに狭い・広い値を許すと数値不安定や逆視点になるので clamp。
pub(crate) const MIN_DIAGONAL_FOV_DEG: f32 = 1.0;
pub(crate) const MAX_DIAGONAL_FOV_DEG: f32 = 160.0;

impl Default for OrbitCamera {
	fn default() -> Self {
		let target = INITIAL_CAMERA_TARGET;
		let initial_pos = INITIAL_CAMERA_POS_POSITIVE_Z;
		let offset = initial_pos - target;
		let radius = offset.length().max(0.1);
		let latitude = (offset.y / radius).asin();
		let longitude = offset.x.atan2(-offset.z);
		Self {
			target,
			longitude,
			latitude,
			radius,
			diagonal_fov_deg: DEFAULT_DIAGONAL_FOV_DEG,
		}
	}
}

impl OrbitCamera {
	const MIN_LATITUDE: f32 = -1.52;
	const MAX_LATITUDE: f32 = 1.52;
	const MIN_RADIUS: f32 = 0.03;
	const MAX_RADIUS: f32 = 30.0;

	pub(crate) fn position(self) -> Vec3 {
		let cos_lat = self.latitude.cos();
		let offset = Vec3::new(
			self.radius * cos_lat * self.longitude.sin(),
			self.radius * self.latitude.sin(),
			-self.radius * cos_lat * self.longitude.cos(),
		);
		self.target + offset
	}

	pub(crate) fn orbit(&mut self, delta_longitude: f32, delta_latitude: f32) {
		self.longitude += delta_longitude;
		self.latitude = (self.latitude + delta_latitude).clamp(Self::MIN_LATITUDE, Self::MAX_LATITUDE);
	}

	pub(crate) fn set_orbit(&mut self, longitude: Option<f32>, latitude: Option<f32>, radius: Option<f32>) {
		if let Some(longitude) = longitude {
			self.longitude = longitude;
		}
		if let Some(latitude) = latitude {
			self.latitude = latitude.clamp(Self::MIN_LATITUDE, Self::MAX_LATITUDE);
		}
		if let Some(radius) = radius {
			self.radius = radius.clamp(Self::MIN_RADIUS, Self::MAX_RADIUS);
		}
	}

	pub(crate) fn zoom(&mut self, wheel_positive_units: f32) {
		self.radius = (self.radius * (-wheel_positive_units * 0.12).exp()).clamp(Self::MIN_RADIUS, Self::MAX_RADIUS);
	}

	/// カメラから見た right/up 方向に target を平行移動（パン）する。
	/// `screen_dx` / `screen_dy` は画面ピクセル基準で、x はカーソルの右方向、y はカーソルの下方向を正とする。
	/// パン速度は radius にスケールするので、寄っている時は細かく、引いている時は大きく動く。
	pub(crate) fn pan(&mut self, screen_dx: f32, screen_dy: f32) {
		if screen_dx == 0.0 && screen_dy == 0.0 {
			return;
		}
		let pos = self.position();
		let forward = (self.target - pos).normalize_or_zero();
		if forward.length_squared() < 1e-6 {
			return;
		}
		let world_up = Vec3::Y;
		let right = forward.cross(world_up).normalize_or_zero();
		if right.length_squared() < 1e-6 {
			return;
		}
		let up = right.cross(forward).normalize_or_zero();
		// 1080p で半画面ドラッグするとちょうど対象モデル幅程度動くくらいの感度。
		let scale = self.radius * 0.0015;
		self.target += -right * screen_dx * scale + up * screen_dy * scale;
	}

	/// orbit (longitude/latitude) のみを default に戻し、target/radius は保持する。
	pub(crate) fn reset_rotation(&mut self) {
		let def = OrbitCamera::default();
		self.longitude = def.longitude;
		self.latitude = def.latitude;
	}

	/// target（pan 位置）のみを default に戻す。orbit/radius/FOV は保持する。
	/// ミドルダブルクリックでパン操作だけリセットしたい用途。
	pub(crate) fn reset_pan(&mut self) {
		self.target = INITIAL_CAMERA_TARGET;
	}

	/// 対角画角を設定する（範囲外は clamp）。
	pub(crate) fn set_diagonal_fov_deg(&mut self, deg: f32) {
		if deg.is_finite() {
			self.diagonal_fov_deg = deg.clamp(MIN_DIAGONAL_FOV_DEG, MAX_DIAGONAL_FOV_DEG);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_orbit_camera_starts_on_positive_z_side() {
		let camera = OrbitCamera::default();
		let pos = camera.position();
		assert!(pos.z > camera.target.z);
		assert!(pos.abs_diff_eq(INITIAL_CAMERA_POS_POSITIVE_Z, 1e-5));
	}

	#[test]
	fn orbit_camera_allows_close_small_prop_inspection() {
		let mut camera = OrbitCamera::default();
		camera.set_orbit(None, None, Some(0.01));
		assert_eq!(camera.radius, OrbitCamera::MIN_RADIUS);

		camera.radius = 0.05;
		camera.zoom(10.0);
		assert_eq!(camera.radius, OrbitCamera::MIN_RADIUS);
	}
}
