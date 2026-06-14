#[allow(unsafe_code)]
// Windows Shell process identity is an OS boundary. Keep the unsafe call isolated here.
pub(crate) fn set_renderer_app_user_model_id() -> Result<(), String> {
	use windows::{core::HSTRING, Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID};

	let app_id = HSTRING::from("DrUsagi.UNAvatar.Renderer");
	unsafe { SetCurrentProcessExplicitAppUserModelID(&app_id) }.map_err(|error| format!("{error:?}"))
}
