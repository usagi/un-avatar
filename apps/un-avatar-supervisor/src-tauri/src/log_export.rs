//! Non-overwriting diagnostic exports, including simultaneous saves.
use std::{
	fs,
	io::Write,
	path::{Path, PathBuf},
};

pub(crate) fn save(dir: &Path, content: &str, prefix: &str, timestamp: u64) -> Result<PathBuf, String> {
	fs::create_dir_all(dir).map_err(|e| format!("create logs dir: {e}"))?;
	let prefix: String = prefix.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(80).collect();
	let prefix = if prefix.is_empty() { "supervisor" } else { &prefix };
	for suffix in 0..10_000 {
		let name = if suffix == 0 {
			format!("{prefix}-{timestamp}.txt")
		} else {
			format!("{prefix}-{timestamp}-{suffix}.txt")
		};
		let path = dir.join(name);
		match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
			Ok(mut file) => {
				file.write_all(content.as_bytes())
					.and_then(|_| file.sync_all())
					.map_err(|e| format!("write logs {}: {e}", path.display()))?;
				return Ok(path);
			}
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
			Err(e) => return Err(format!("create logs {}: {e}", path.display())),
		}
	}
	Err("too many log exports for this timestamp".into())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn simultaneous_saves_preserve_every_log() {
		let dir = std::env::temp_dir().join(format!(
			"un-log-export-{}-{}",
			std::process::id(),
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		));
		fs::create_dir_all(&dir).unwrap();
		fs::write(dir.join("supervisor-42.txt"), "existing").unwrap();
		let threads: Vec<_> = (0..8)
			.map(|i| {
				let dir = dir.clone();
				std::thread::spawn(move || {
					let text = format!("log-{i}");
					let path = save(&dir, &text, "../", 42).unwrap();
					(path, text)
				})
			})
			.collect();
		let rows: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
		assert_eq!(fs::read_to_string(dir.join("supervisor-42.txt")).unwrap(), "existing");
		for (path, text) in &rows {
			assert_eq!(path.parent(), Some(dir.as_path()));
			assert_eq!(fs::read_to_string(path).unwrap(), *text);
		}
		assert_eq!(fs::read_dir(&dir).unwrap().count(), 9);
		fs::remove_dir_all(dir).unwrap();
	}
	#[test]
	fn directory_failure_is_reported_without_overwriting_file() {
		let path = std::env::temp_dir().join(format!("un-log-failure-{}", std::process::id()));
		fs::write(&path, "keep").unwrap();
		assert!(save(&path, "new", "test", 42).is_err());
		assert_eq!(fs::read_to_string(&path).unwrap(), "keep");
		fs::remove_file(path).unwrap();
	}
}
