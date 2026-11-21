//! Observe an attached process without acquiring permission to terminate it.
// Windows handle FFI is confined here; no termination rights or raw handles escape.
#![cfg_attr(windows, allow(unsafe_code))]
use std::time::Instant;

#[cfg(windows)]
pub struct ProcessExit(std::os::windows::io::OwnedHandle);
#[cfg(not(windows))]
pub struct ProcessExit;

impl ProcessExit {
	pub fn open(pid: u32) -> Result<Self, String> {
		if pid == 0 {
			return Err("Renderer PID is unavailable".into());
		}
		#[cfg(windows)]
		{
			use std::os::windows::io::FromRawHandle;
			use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
			// SAFETY: scalar PID and access flags; successful handle is owned by Self.
			unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
				.map(|handle| Self(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle.0) }))
				.map_err(|e| format!("observe Renderer process {pid}: {e}"))
		}
		#[cfg(not(windows))]
		{
			Err("Attached Renderer exit observation is not available on this platform".into())
		}
	}
	pub fn wait_until(&self, deadline: Instant) -> Result<(), String> {
		#[cfg(windows)]
		{
			use std::os::windows::io::AsRawHandle;
			use windows::Win32::{
				Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
				System::Threading::WaitForSingleObject,
			};
			let millis = deadline
				.saturating_duration_since(Instant::now())
				.as_millis()
				.min(u32::MAX as u128 - 1) as u32;
			// SAFETY: Self owns a valid open handle until Drop; wait does not close it.
			match unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), millis) } {
				WAIT_OBJECT_0 => Ok(()),
				WAIT_TIMEOUT => Err("Renderer has not exited; it remains managed for retry".into()),
				_ => Err(format!("observe Renderer exit: {}", windows::core::Error::from_win32())),
			}
		}
		#[cfg(not(windows))]
		{
			let _ = deadline;
			Err("Attached Renderer exit observation is not available on this platform".into())
		}
	}
}

#[cfg(all(test, windows))]
mod tests {
	use super::*;
	use std::{os::windows::process::CommandExt, process::Command, time::Duration};
	#[test]
	fn observation_does_not_terminate_and_retains_original_process_handle() {
		let mut child = Command::new("pwsh")
			.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 1"])
			.creation_flags(0x08000000)
			.spawn()
			.unwrap();
		let observer = ProcessExit::open(child.id()).unwrap();
		assert!(observer.wait_until(Instant::now()).is_err());
		assert!(child.try_wait().unwrap().is_none());
		assert!(observer.wait_until(Instant::now() + Duration::from_secs(15)).is_ok());
		assert!(child.wait().unwrap().success());
		assert!(observer.wait_until(Instant::now()).is_ok());
		assert!(ProcessExit::open(0).is_err());
	}
}
