//! Recoverable multi-file profile publication. Caller holds the profile-store lock.
//! This storage primitive does not reload renderers or resolve profile destinations.
use serde::{Deserialize, Serialize};
use std::{
	fs, io,
	path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
	pub name: String,
	pub before: Option<Vec<u8>>,
	pub after: Option<Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
	version: u32,
	root: PathBuf,
	changes: Vec<Change>,
}
#[derive(Debug, Serialize)]
pub(crate) struct HistoryEntry {
	pub name: String,
	pub revision: Option<String>,
	pub files: Vec<String>,
	pub state: String,
	pub error: Option<String>,
}
fn journal_path(root: &Path, name: &str) -> io::Result<PathBuf> {
	if name.contains(['/', '\\', ':']) || !name.starts_with(".un-avatar-batch-") || !name.ends_with(".json") {
		return Err(invalid("invalid batch journal name"));
	}
	Ok(root.join(name))
}
fn inspect(root: &Path, name: &str) -> io::Result<HistoryEntry> {
	let path = journal_path(root, name)?;
	let meta = fs::symlink_metadata(&path)?;
	if !meta.is_file() || meta.file_type().is_symlink() {
		return Err(invalid("invalid batch journal file"));
	}
	let bytes = fs::read(path)?;
	let record: Record = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
	if record.version != 1 || record.root != root || record.changes.is_empty() {
		return Err(invalid("batch journal identity changed"));
	}
	let mut names = std::collections::BTreeSet::new();
	let mut original = true;
	let mut applied = true;
	let mut conflict = false;
	for change in &record.changes {
		if !names.insert(change.name.to_lowercase()) {
			return Err(invalid("duplicate journal target"));
		}
		let current = contents(&target(root, &change.name)?)?;
		original &= current == change.before;
		applied &= current == change.after;
		conflict |= current != change.before && current != change.after;
	}
	Ok(HistoryEntry {
		name: name.into(),
		revision: Some(crate::profile_write::revision(&bytes)),
		files: record.changes.iter().map(|c| c.name.clone()).collect(),
		state: if conflict {
			"conflict"
		} else if original {
			"original"
		} else if applied {
			"applied"
		} else {
			"partial"
		}
		.into(),
		error: None,
	})
}
pub(crate) fn history(root: &Path) -> io::Result<Vec<HistoryEntry>> {
	let root = root.canonicalize()?;
	let mut entries = vec![];
	for entry in fs::read_dir(&root)? {
		let entry = entry?;
		let name = entry.file_name().to_string_lossy().into_owned();
		if !name.starts_with(".un-avatar-batch-") || !name.ends_with(".json") {
			continue;
		}
		entries.push(inspect(&root, &name).unwrap_or_else(|e| HistoryEntry {
			name,
			revision: None,
			files: vec![],
			state: "invalid".into(),
			error: Some(e.to_string()),
		}));
	}
	entries.sort_by(|a, b| a.name.cmp(&b.name));
	Ok(entries)
}
pub(crate) fn restore_reviewed(root: &Path, name: &str, revision: &str) -> io::Result<()> {
	let root = root.canonicalize()?;
	let entry = inspect(&root, name)?;
	if entry.revision.as_deref() != Some(revision) {
		return Err(invalid("profile_conflict: recovery record changed"));
	}
	if entry.state == "conflict" {
		return Err(invalid("profile_conflict: recovery target edited"));
	}
	restore(&root, &journal_path(&root, name)?)
}
fn invalid(message: &str) -> io::Error {
	io::Error::new(io::ErrorKind::InvalidData, message)
}
fn target(root: &Path, name: &str) -> io::Result<PathBuf> {
	if name.is_empty() || name.contains(['/', '\\', ':']) || name.chars().any(char::is_control) || !name.ends_with(".toml") {
		return Err(invalid("invalid profile batch target"));
	}
	let path = root.join(name);
	if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
		return Err(invalid("batch target must be a regular file"));
	}
	Ok(path)
}
fn contents(path: &Path) -> io::Result<Option<Vec<u8>>> {
	match fs::read(path) {
		Ok(bytes) => Ok(Some(bytes)),
		Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(e),
	}
}
fn write(path: &Path, value: &Option<Vec<u8>>) -> io::Result<()> {
	match value {
		Some(bytes) => {
			if path.exists() {
				crate::profile_write::replace(path, bytes)
			} else {
				crate::profile_write::create(path, bytes)
			}
		}
		None => match fs::remove_file(path) {
			Ok(()) => Ok(()),
			Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
			Err(e) => Err(e),
		},
	}
}
#[derive(Debug)]
pub(crate) struct Outcome {
	pub journal: PathBuf,
	pub completed: usize,
	pub error: Option<String>,
}
pub(crate) fn apply(root: &Path, changes: Vec<Change>) -> io::Result<Outcome> {
	apply_with(root, changes, |_| Ok(()))
}
fn apply_with(root: &Path, changes: Vec<Change>, mut checkpoint: impl FnMut(usize) -> io::Result<()>) -> io::Result<Outcome> {
	let root = root.canonicalize()?;
	let mut names = std::collections::BTreeSet::new();
	if changes.is_empty() {
		return Err(invalid("empty batch"));
	}
	for change in &changes {
		if !names.insert(change.name.to_lowercase()) {
			return Err(invalid("duplicate batch target"));
		}
		if contents(&target(&root, &change.name)?)? != change.before {
			return Err(invalid("profile_conflict: batch source changed"));
		}
	}
	let record = Record {
		version: 1,
		root: root.clone(),
		changes,
	};
	let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
	let journal = root.join(format!(
		".un-avatar-batch-{}-{}.json",
		std::process::id(),
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_err(io::Error::other)?
			.as_nanos()
	));
	crate::profile_write::create(&journal, &bytes)?;
	let mut result = Outcome {
		journal,
		completed: 0,
		error: None,
	};
	for (i, change) in record.changes.iter().enumerate() {
		let operation = (|| {
			checkpoint(i)?;
			let path = target(&root, &change.name)?;
			if contents(&path)? != change.before {
				return Err(invalid("profile_conflict: batch target changed"));
			}
			write(&path, &change.after)
		})();
		if let Err(error) = operation {
			result.error = Some(error.to_string());
			break;
		}
		result.completed += 1;
	}
	Ok(result)
}
pub(crate) fn restore(root: &Path, journal: &Path) -> io::Result<()> {
	let root = root.canonicalize()?;
	if journal.parent() != Some(root.as_path())
		|| !journal
			.file_name()
			.and_then(|n| n.to_str())
			.is_some_and(|n| n.starts_with(".un-avatar-batch-") && n.ends_with(".json"))
	{
		return Err(invalid("invalid batch journal path"));
	}
	let metadata = fs::symlink_metadata(journal)?;
	if !metadata.is_file() || metadata.file_type().is_symlink() {
		return Err(invalid("invalid batch journal file"));
	}
	let record: Record = serde_json::from_slice(&fs::read(journal)?).map_err(io::Error::other)?;
	if record.version != 1 || record.root != root || record.changes.is_empty() {
		return Err(invalid("batch journal identity changed"));
	}
	let mut names = std::collections::BTreeSet::new();
	for change in &record.changes {
		if !names.insert(change.name.to_lowercase()) {
			return Err(invalid("duplicate journal target"));
		}
		let current = contents(&target(&root, &change.name)?)?;
		if current != change.before && current != change.after {
			return Err(invalid("profile_conflict: recovery target edited"));
		}
	}
	for change in record.changes.iter().rev() {
		let path = target(&root, &change.name)?;
		if contents(&path)? != change.before {
			write(&path, &change.before)?;
		}
	}
	Ok(())
}
#[cfg(test)]
mod tests {
	use super::*;
	#[cfg(windows)]
	#[test]
	fn restore_write_failure_can_resume_in_fresh_process() {
		use std::os::windows::fs::OpenOptionsExt;
		let temp = tempfile::tempdir().unwrap();
		let root = temp.path().canonicalize().unwrap();
		let lock = crate::profile_write::lock_store(&root).unwrap();
		for name in ["a.toml", "b.toml", "unrelated.toml"] {
			fs::write(root.join(name), b"original").unwrap();
		}
		let changes = ["a.toml", "b.toml"]
			.into_iter()
			.map(|name| Change {
				name: name.into(),
				before: Some(b"original".to_vec()),
				after: Some(b"updated".to_vec()),
			})
			.collect();
		let outcome = apply(&root, changes).unwrap();
		assert!(outcome.error.is_none());
		let journal_bytes = fs::read(&outcome.journal).unwrap();
		// Reads succeed during preflight; replacement fails after b has been restored.
		let denied = fs::OpenOptions::new().read(true).share_mode(1).open(root.join("a.toml")).unwrap();
		assert!(restore(&root, &outcome.journal).is_err());
		assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"updated");
		assert_eq!(fs::read(root.join("b.toml")).unwrap(), b"original");
		assert_eq!(history(&root).unwrap()[0].state, "partial");
		assert_eq!(fs::read(&outcome.journal).unwrap(), journal_bytes);
		drop(denied);
		drop(lock);
		for _ in 0..2 {
			let restored = run_worker(&root, "restore");
			assert!(restored.status.success(), "{}", String::from_utf8_lossy(&restored.stderr));
		}
		for name in ["a.toml", "b.toml", "unrelated.toml"] {
			assert_eq!(fs::read(root.join(name)).unwrap(), b"original");
		}
		assert_eq!(history(&root).unwrap()[0].state, "original");
		assert_eq!(fs::read(&outcome.journal).unwrap(), journal_bytes);
	}
	#[test]
	fn batch_process_worker() {
		let Some(root) = std::env::var_os("UNA_BATCH_TEST_ROOT") else {
			return;
		};
		let root = PathBuf::from(root);
		let mode = std::env::var("UNA_BATCH_TEST_MODE").unwrap();
		let _lock = crate::profile_write::lock_store(&root).unwrap();
		if mode == "restore" {
			let entries = history(&root).unwrap();
			assert_eq!(entries.len(), 1);
			let entry = &entries[0];
			restore_reviewed(&root, &entry.name, entry.revision.as_deref().unwrap()).unwrap();
			return;
		}
		let stop: usize = mode.parse().unwrap();
		let changes = vec![
			Change {
				name: "new.toml".into(),
				before: None,
				after: Some(b"original".to_vec()),
			},
			Change {
				name: "old.toml".into(),
				before: Some(b"original".to_vec()),
				after: None,
			},
			Change {
				name: "other.toml".into(),
				before: Some(b"original".to_vec()),
				after: Some(b"updated".to_vec()),
			},
		];
		let result = apply_with(&root, changes, |index| {
			if index == stop {
				std::process::exit(74);
			}
			Ok(())
		})
		.unwrap();
		assert!(result.error.is_none());
		std::process::exit(74);
	}
	fn run_worker(root: &Path, mode: &str) -> std::process::Output {
		let name = format!("{}::batch_process_worker", module_path!().split_once("::").unwrap().1);
		let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
		cmd.args(["--exact", &name])
			.env("UNA_BATCH_TEST_ROOT", root)
			.env("UNA_BATCH_TEST_MODE", mode);
		#[cfg(windows)]
		{
			use std::os::windows::process::CommandExt;
			cmd.creation_flags(0x08000000);
		}
		cmd.output().unwrap()
	}
	#[test]
	fn batch_process_exit_restores_from_disk_in_new_process() {
		for stop in 0..=3 {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path().canonicalize().unwrap();
			for name in ["old.toml", "other.toml", "untouched.toml"] {
				fs::write(root.join(name), b"original").unwrap();
			}
			let exited = run_worker(&root, &stop.to_string());
			assert_eq!(exited.status.code(), Some(74), "{}", String::from_utf8_lossy(&exited.stderr));
			assert_eq!(root.join("new.toml").exists(), stop >= 1);
			assert_eq!(root.join("old.toml").exists(), stop < 2);
			assert_eq!(
				fs::read(root.join("other.toml")).unwrap(),
				if stop == 3 { b"updated".as_slice() } else { b"original".as_slice() }
			);
			assert_eq!(
				history(&root).unwrap()[0].state,
				if stop == 0 {
					"original"
				} else if stop == 3 {
					"applied"
				} else {
					"partial"
				}
			);
			for _ in 0..2 {
				let restored = run_worker(&root, "restore");
				assert!(
					restored.status.success(),
					"{}{}",
					String::from_utf8_lossy(&restored.stdout),
					String::from_utf8_lossy(&restored.stderr)
				);
				assert!(String::from_utf8_lossy(&restored.stdout).contains("1 passed"));
			}
			for name in ["old.toml", "other.toml", "untouched.toml"] {
				assert_eq!(fs::read(root.join(name)).unwrap(), b"original");
			}
			assert!(!root.join("new.toml").exists());
			assert_eq!(history(&root).unwrap()[0].state, "original");
		}
	}
	#[test]
	fn history_distinguishes_partial_complete_restored_and_edited() {
		let temp = tempfile::tempdir().unwrap();
		let root = temp.path().canonicalize().unwrap();
		fs::write(root.join("a.toml"), b"old").unwrap();
		fs::write(root.join("b.toml"), b"old").unwrap();
		let changes = ["a.toml", "b.toml"]
			.into_iter()
			.map(|name| Change {
				name: name.into(),
				before: Some(b"old".to_vec()),
				after: Some(b"new".to_vec()),
			})
			.collect();
		let result = apply_with(&root, changes, |i| if i == 1 { Err(io::Error::other("stop")) } else { Ok(()) }).unwrap();
		let entry = history(&root).unwrap().remove(0);
		assert_eq!(entry.state, "partial");
		let revision = entry.revision.unwrap();
		fs::write(root.join("b.toml"), b"new").unwrap();
		assert_eq!(history(&root).unwrap()[0].state, "applied");
		fs::write(root.join("b.toml"), b"user edit").unwrap();
		assert_eq!(history(&root).unwrap()[0].state, "conflict");
		assert!(restore_reviewed(&root, &entry.name, &revision).is_err());
		assert_eq!(fs::read(root.join("a.toml")).unwrap(), b"new");
		fs::write(root.join("b.toml"), b"new").unwrap();
		restore_reviewed(&root, &entry.name, &revision).unwrap();
		assert_eq!(history(&root).unwrap()[0].state, "original");
		let mut bytes = fs::read(&result.journal).unwrap();
		bytes.push(b' ');
		fs::write(&result.journal, bytes).unwrap();
		assert!(restore_reviewed(&root, &entry.name, &revision).is_err());
		fs::write(&result.journal, b"broken").unwrap();
		assert_eq!(history(&root).unwrap()[0].state, "invalid");
	}
	#[test]
	fn batch_failure_restores_replace_create_and_rename_without_losing_extensions() {
		for stop in 0..=4 {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path().canonicalize().unwrap();
			let original = b"[custom]\nvalue='preserved'\n".to_vec();
			fs::write(root.join("old.toml"), &original).unwrap();
			fs::write(root.join("other.toml"), &original).unwrap();
			let changes = vec![
				Change {
					name: "new.toml".into(),
					before: None,
					after: Some(original.clone()),
				},
				Change {
					name: "old.toml".into(),
					before: Some(original.clone()),
					after: None,
				},
				Change {
					name: "other.toml".into(),
					before: Some(original.clone()),
					after: Some(b"updated".to_vec()),
				},
				Change {
					name: "seed.toml".into(),
					before: None,
					after: Some(b"seed copy".to_vec()),
				},
			];
			let result = apply_with(&root, changes, |index| {
				if index == stop {
					Err(io::Error::other("injected publication failure"))
				} else {
					Ok(())
				}
			})
			.unwrap();
			assert_eq!(result.completed, stop);
			assert_eq!(result.error.is_some(), stop < 4);
			restore(&root, &result.journal).unwrap();
			restore(&root, &result.journal).unwrap();
			assert_eq!(fs::read(root.join("old.toml")).unwrap(), original);
			assert_eq!(fs::read(root.join("other.toml")).unwrap(), original);
			assert!(!root.join("new.toml").exists());
			assert!(!root.join("seed.toml").exists());
			fs::write(root.join("other.toml"), b"user edit").unwrap();
			assert!(restore(&root, &result.journal).is_err());
			assert_eq!(fs::read(root.join("other.toml")).unwrap(), b"user edit");
		}
	}
}
