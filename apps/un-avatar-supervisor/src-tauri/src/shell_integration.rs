//! Optional non-registering startup for portable and automated use.
//! This does not isolate profiles, network ports, or rendering resources.

pub(crate) fn disabled() -> bool {
	std::env::args_os().skip(1).any(|arg| arg == "--no-shell-integration")
}

pub(crate) fn require_enabled() -> Result<(), String> {
	if disabled() {
		Err("Windows連携を無効にして起動しています。この操作を使う場合は --no-shell-integration を外して起動し直してください。".into())
	} else {
		Ok(())
	}
}
