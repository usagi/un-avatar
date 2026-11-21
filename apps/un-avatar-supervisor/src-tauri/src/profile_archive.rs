//! File storage for recoverable profile deletion. Lifecycle integration must hold
//! the same exclusion as launch/edit/delete and update seed visibility separately.
//! Creating an archive never deletes a source profile.
use serde::{Deserialize, Serialize};
use std::{
	fs::{self, OpenOptions},
	io::{self, Write},
	path::{Component, Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
	time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Origin {
	User,
	Seed,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchivedProfile {
	pub version: u32,
	pub profile_id: String,
	pub name: String,
	pub original_filename: String,
	pub origin: Origin,
	pub seed_was_hidden: bool,
	pub manifest: Vec<u8>,
}
#[derive(Debug)]
pub struct ArchiveEntry {
	pub archive_id: String,
	pub profile: ArchivedProfile,
}

fn invalid(message: &str) -> io::Error {
	io::Error::new(io::ErrorKind::InvalidData, message)
}
fn single_filename(value: &str) -> io::Result<()> {
	let mut parts = Path::new(value).components();
	if value.is_empty() || value.contains(['/', '\\', ':']) || !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some()
	{
		return Err(invalid("expected a single filename"));
	}
	Ok(())
}
fn validate(profile: &ArchivedProfile) -> io::Result<()> {
	if profile.version != 1 || profile.profile_id.trim().is_empty() {
		return Err(invalid("unsupported or invalid profile archive"));
	}
	single_filename(&profile.original_filename)?;
	if Path::new(&profile.original_filename).extension().and_then(|x| x.to_str()) != Some("toml") {
		return Err(invalid("profile filename must end in .toml"));
	}
	Ok(())
}
fn unique_name() -> String {
	format!(
		"{}-{}-{}",
		std::process::id(),
		SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(),
		NEXT.fetch_add(1, Ordering::Relaxed)
	)
}
/// Publish complete bytes without replacing any destination. The temporary file
/// resides in the destination directory, so publication is on the same volume.
/// If hard links are unavailable, return an error rather than fall back to overwrite.
fn publish_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
	let parent = path.parent().ok_or_else(|| invalid("missing parent directory"))?;
	fs::create_dir_all(parent)?;
	let temporary = parent.join(format!(".profile-stage-{}.tmp", unique_name()));
	let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
	let result = (|| {
		file.write_all(bytes)?;
		file.sync_all()?;
		fs::hard_link(&temporary, path)
	})();
	drop(file);
	// Publication succeeded even if cleanup fails; a .tmp is never an archive.
	let _ = fs::remove_file(&temporary);
	result
}
/// Publish a new managed icon; never overwrite assets referenced by another
/// profile, a duplicate, or an archived manifest. Reference-aware cleanup is a
/// separate operation; a failed manifest save may leave an unreferenced image.
pub fn store_icon(root: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
	let destination = root.join(format!("icon-{}.webp", unique_name()));
	publish_new(&destination, bytes)?;
	Ok(destination)
}

pub fn store(root: &Path, profile: &ArchivedProfile) -> io::Result<String> {
	validate(profile)?;
	let bytes = serde_json::to_vec(profile).map_err(io::Error::other)?;
	let archive_id = format!("{}.json", unique_name());
	publish_new(&root.join(&archive_id), &bytes)?;
	Ok(archive_id)
}
pub fn load(root: &Path, archive_id: &str) -> io::Result<ArchivedProfile> {
	single_filename(archive_id)?;
	if Path::new(archive_id).extension().and_then(|x| x.to_str()) != Some("json") {
		return Err(invalid("invalid archive ID"));
	}
	let profile: ArchivedProfile = serde_json::from_slice(&fs::read(root.join(archive_id))?).map_err(io::Error::other)?;
	validate(&profile)?;
	Ok(profile)
}
pub fn list(root: &Path) -> io::Result<Vec<ArchiveEntry>> {
	let entries = match fs::read_dir(root) {
		Ok(entries) => entries,
		Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
		Err(e) => return Err(e),
	};
	let mut result = vec![];
	for entry in entries {
		let path = entry?.path();
		if path.extension().and_then(|x| x.to_str()) != Some("json") {
			continue;
		}
		let id = path
			.file_name()
			.and_then(|x| x.to_str())
			.ok_or_else(|| invalid("invalid archive filename"))?;
		result.push(ArchiveEntry {
			archive_id: id.to_owned(),
			profile: load(root, id)?,
		});
	}
	result.sort_by(|a, b| a.archive_id.cmp(&b.archive_id));
	Ok(result)
}
/// Restore a user manifest only. Caller must reject an active duplicate profile
/// ID under its lifecycle lock before calling this function. Seed visibility is
/// a separate transaction and must not be treated as user-file restoration.
/// Keep the archive after restoration, including on failure.
pub fn restore_user(root: &Path, archive_id: &str, profiles: &Path) -> io::Result<PathBuf> {
	let profile = load(root, archive_id)?;
	if profile.origin != Origin::User {
		return Err(invalid("seed archive requires visibility restoration"));
	}
	let destination = profiles.join(&profile.original_filename);
	publish_new(&destination, &profile.manifest)?;
	Ok(destination)
}

static VISIBILITY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Missing storage means no hidden seeds; corrupt or unreadable storage is an
/// error, never an empty list that a subsequent change may overwrite.
pub fn hidden_seed_ids(path: &Path) -> io::Result<Vec<String>> {
	let bytes = match fs::read(path) {
		Ok(bytes) => bytes,
		Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
		Err(e) => return Err(e),
	};
	serde_json::from_slice(&bytes).map_err(io::Error::other)
}
/// Read/modify/write exclusion covers callers in this Supervisor. Lifecycle
/// integration must separately coordinate processes and profile file changes.
pub fn set_seed_hidden(path: &Path, id: &str, hidden: bool) -> io::Result<()> {
	let _guard = VISIBILITY_LOCK
		.lock()
		.map_err(|_| io::Error::other("seed visibility lock poisoned"))?;
	set_seed_hidden_locked(path, id, hidden)
}
fn set_seed_hidden_locked(path: &Path, id: &str, hidden: bool) -> io::Result<()> {
	let mut ids = hidden_seed_ids(path)?;
	let exists = ids.iter().any(|value| value == id);
	if exists == hidden {
		return Ok(());
	}
	if hidden {
		ids.push(id.to_owned());
	} else {
		ids.retain(|value| value != id);
	}
	ids.sort();
	ids.dedup();
	let bytes = serde_json::to_vec_pretty(&ids).map_err(io::Error::other)?;
	let parent = path.parent().ok_or_else(|| invalid("missing visibility directory"))?;
	fs::create_dir_all(parent)?;
	let temporary = parent.join(format!(".visibility-stage-{}.tmp", unique_name()));
	let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
	let result = (|| {
		file.write_all(&bytes)?;
		file.sync_all()
	})();
	drop(file);
	let result = result.and_then(|()| fs::rename(&temporary, path));
	let _ = fs::remove_file(&temporary);
	result
}

/// Caller must already have durably archived `expected`, and coordinate profile
/// edits/launch with deletion. The byte check detects prior changes; it is not
/// an inter-process compare-and-delete primitive.
pub fn remove_archived_user(manifest: &Path, expected: &[u8], visibility: &Path, id: &str, hide_seed: bool) -> io::Result<()> {
	remove_archived_user_with(manifest, expected, visibility, id, hide_seed, |path| fs::remove_file(path))
}
fn remove_archived_user_with(
	manifest: &Path,
	expected: &[u8],
	visibility: &Path,
	id: &str,
	hide_seed: bool,
	remove: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
	let _guard = VISIBILITY_LOCK
		.lock()
		.map_err(|_| io::Error::other("seed visibility lock poisoned"))?;
	if fs::read(manifest)? != expected {
		return Err(io::Error::other("profile changed after archive; deletion refused"));
	}
	let was_hidden = if hide_seed {
		hidden_seed_ids(visibility)?.iter().any(|value| value == id)
	} else {
		false
	};
	if hide_seed {
		set_seed_hidden_locked(visibility, id, true)?;
	}
	if let Err(error) = remove(manifest) {
		if hide_seed && !was_hidden {
			if let Err(rollback) = set_seed_hidden_locked(visibility, id, false) {
				return Err(io::Error::other(format!(
					"delete failed: {error}; seed visibility rollback also failed: {rollback}; archive retained"
				)));
			}
		}
		return Err(error);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	struct Sandbox(PathBuf);
	impl Sandbox {
		fn new() -> Self {
			let p = std::env::temp_dir().join(format!("una-archive-test-{}", unique_name()));
			fs::create_dir(&p).unwrap();
			Self(p)
		}
	}
	impl Drop for Sandbox {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}
	fn fixture() -> ArchivedProfile {
		ArchivedProfile {
			version: 1,
			profile_id: "profile-a".into(),
			name: "Avatar A".into(),
			original_filename: "a.toml".into(),
			origin: Origin::User,
			seed_was_hidden: false,
			manifest: b"# preserve comments\r\nname = \"A\"\r\n".to_vec(),
		}
	}
	#[test]
	fn roundtrip_keeps_original_bytes_and_archive() {
		let s = Sandbox::new();
		let root = s.0.join("archive");
		let profiles = s.0.join("profiles");
		fs::create_dir(&profiles).unwrap();
		let p = fixture();
		fs::write(profiles.join("a.toml"), &p.manifest).unwrap();
		fs::write(profiles.join("b.toml"), b"unrelated").unwrap();
		let id = store(&root, &p).unwrap();
		assert_eq!(fs::read(profiles.join("a.toml")).unwrap(), p.manifest);
		// Simulate the separately coordinated deletion, only inside this sandbox.
		fs::remove_file(profiles.join("a.toml")).unwrap();
		let restored = restore_user(&root, &id, &profiles).unwrap();
		assert_eq!(fs::read(restored).unwrap(), p.manifest);
		assert_eq!(fs::read(profiles.join("b.toml")).unwrap(), b"unrelated");
		assert_eq!(load(&root, &id).unwrap(), p);
		assert_eq!(list(&root).unwrap().len(), 1);
	}
	#[test]
	fn conflict_never_overwrites_and_keeps_archive() {
		let s = Sandbox::new();
		let p = fixture();
		let root = s.0.join("archive");
		let id = store(&root, &p).unwrap();
		let profiles = s.0.join("profiles");
		fs::create_dir(&profiles).unwrap();
		fs::write(profiles.join("a.toml"), b"new profile").unwrap();
		assert!(restore_user(&root, &id, &profiles).is_err());
		assert_eq!(fs::read(profiles.join("a.toml")).unwrap(), b"new profile");
		assert_eq!(load(&root, &id).unwrap(), p);
	}
	#[test]
	fn corrupt_and_unsupported_archives_fail_closed() {
		let s = Sandbox::new();
		fs::write(s.0.join("bad.json"), b"broken").unwrap();
		assert!(list(&s.0).is_err());
		let mut p = fixture();
		p.version = 2;
		assert!(store(&s.0, &p).is_err());
		fs::write(s.0.join("future.json"), serde_json::to_vec(&p).unwrap()).unwrap();
		assert!(load(&s.0, "future.json").is_err());
	}
	#[test]
	fn paths_and_seed_restore_cannot_escape_user_contract() {
		let s = Sandbox::new();
		let mut p = fixture();
		for name in ["../a.toml", "..\\a.toml", "C:a.toml", "/a.toml", "a.txt"] {
			p.original_filename = name.into();
			assert!(store(&s.0, &p).is_err());
		}
		assert!(load(&s.0, "../a.json").is_err());
		p = fixture();
		p.origin = Origin::Seed;
		let id = store(&s.0, &p).unwrap();
		assert!(restore_user(&s.0, &id, &s.0.join("profiles")).is_err());
		assert_eq!(load(&s.0, &id).unwrap(), p);
	}
	#[test]
	fn archive_write_failure_does_not_touch_source() {
		let s = Sandbox::new();
		let p = fixture();
		let source = s.0.join("a.toml");
		fs::write(&source, &p.manifest).unwrap();
		let root = s.0.join("not-a-directory");
		fs::write(&root, b"occupied").unwrap();
		assert!(store(&root, &p).is_err());
		assert_eq!(fs::read(source).unwrap(), p.manifest);
	}
	#[test]
	fn simultaneous_restores_publish_only_one_complete_file() {
		use std::sync::{Arc, Barrier};
		let s = Sandbox::new();
		let root = s.0.join("archive");
		let profiles = s.0.join("profiles");
		let first = fixture();
		let mut second = fixture();
		second.profile_id = "other-id".into();
		second.manifest = b"name = \"B\"\n".to_vec();
		let ids = [store(&root, &first).unwrap(), store(&root, &second).unwrap()];
		let barrier = Arc::new(Barrier::new(2));
		let handles: Vec<_> = ids
			.into_iter()
			.map(|id| {
				let root = root.clone();
				let profiles = profiles.clone();
				let barrier = barrier.clone();
				std::thread::spawn(move || {
					barrier.wait();
					restore_user(&root, &id, &profiles)
				})
			})
			.collect();
		let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
		assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
		let bytes = fs::read(profiles.join("a.toml")).unwrap();
		assert!(bytes == first.manifest || bytes == second.manifest);
		assert_eq!(list(&root).unwrap().len(), 2);
		assert_eq!(fs::read_dir(&profiles).unwrap().count(), 1);
	}
	#[test]
	fn seed_visibility_roundtrip_preserves_other_ids() {
		let s = Sandbox::new();
		let path = s.0.join("hidden.json");
		set_seed_hidden(&path, "other", true).unwrap();
		set_seed_hidden(&path, "a", true).unwrap();
		assert_eq!(hidden_seed_ids(&path).unwrap(), vec!["a", "other"]);
		set_seed_hidden(&path, "a", false).unwrap();
		assert_eq!(hidden_seed_ids(&path).unwrap(), vec!["other"]);
		set_seed_hidden(&path, "a", false).unwrap();
		assert_eq!(hidden_seed_ids(&path).unwrap(), vec!["other"]);
	}
	#[test]
	fn corrupt_visibility_is_not_replaced_with_empty_state() {
		let s = Sandbox::new();
		let path = s.0.join("hidden.json");
		fs::write(&path, b"bad data").unwrap();
		assert!(hidden_seed_ids(&path).is_err());
		assert!(set_seed_hidden(&path, "a", true).is_err());
		assert_eq!(fs::read(&path).unwrap(), b"bad data");
	}
	#[test]
	fn concurrent_seed_updates_do_not_lose_other_ids() {
		let s = Sandbox::new();
		let path = s.0.join("hidden.json");
		let handles: Vec<_> = (0..8)
			.map(|n| {
				let path = path.clone();
				std::thread::spawn(move || set_seed_hidden(&path, &format!("p{n}"), true))
			})
			.collect();
		for h in handles {
			h.join().unwrap().unwrap();
		}
		assert_eq!(hidden_seed_ids(&path).unwrap(), (0..8).map(|n| format!("p{n}")).collect::<Vec<_>>());
	}
	#[test]
	fn failed_removal_restores_seed_visibility_and_retains_profile() {
		let s = Sandbox::new();
		let path = s.0.join("a.toml");
		let visibility = s.0.join("hidden.json");
		fs::write(&path, b"original").unwrap();
		set_seed_hidden(&visibility, "other", true).unwrap();
		let result = remove_archived_user_with(&path, b"original", &visibility, "a", true, |_| {
			Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected removal failure"))
		});
		assert!(result.is_err());
		assert_eq!(fs::read(&path).unwrap(), b"original");
		assert_eq!(hidden_seed_ids(&visibility).unwrap(), vec!["other"]);
	}
	#[test]
	fn changed_profile_is_not_removed_or_hidden() {
		let s = Sandbox::new();
		let path = s.0.join("a.toml");
		let visibility = s.0.join("hidden.json");
		fs::write(&path, b"new edit").unwrap();
		assert!(remove_archived_user(&path, b"old snapshot", &visibility, "a", true).is_err());
		assert_eq!(fs::read(path).unwrap(), b"new edit");
		assert!(!visibility.exists());
	}
	#[test]
	fn successful_removal_keeps_archive_and_hides_seed_only() {
		let s = Sandbox::new();
		let p = fixture();
		let path = s.0.join("a.toml");
		let visibility = s.0.join("hidden.json");
		let root = s.0.join("archive");
		fs::write(&path, &p.manifest).unwrap();
		let id = store(&root, &p).unwrap();
		set_seed_hidden(&visibility, "other", true).unwrap();
		remove_archived_user(&path, &p.manifest, &visibility, &p.profile_id, true).unwrap();
		assert!(!path.exists());
		assert_eq!(hidden_seed_ids(&visibility).unwrap(), vec!["other", "profile-a"]);
		assert_eq!(load(&root, &id).unwrap(), p);
	}
	#[test]
	fn removal_failure_preserves_previously_hidden_seed() {
		let s = Sandbox::new();
		let path = s.0.join("a.toml");
		let visibility = s.0.join("hidden.json");
		fs::write(&path, b"original").unwrap();
		set_seed_hidden(&visibility, "a", true).unwrap();
		assert!(remove_archived_user_with(&path, b"original", &visibility, "a", true, |_| Err(io::Error::other("failed"))).is_err());
		assert_eq!(hidden_seed_ids(&visibility).unwrap(), vec!["a"]);
	}
	#[test]
	fn later_icon_save_does_not_change_archived_icon_reference() {
		let s = Sandbox::new();
		let icons = s.0.join("thumbnails");
		let archives = s.0.join("archive");
		let old = store_icon(&icons, b"old image bytes").unwrap();
		let mut p = fixture();
		p.manifest = format!("[window]\nicon_path = {:?}\n", old.to_string_lossy()).into_bytes();
		let id = store(&archives, &p).unwrap();
		let new = store_icon(&icons, b"new image bytes").unwrap();
		assert_ne!(old, new);
		assert_eq!(fs::read(&old).unwrap(), b"old image bytes");
		assert_eq!(fs::read(new).unwrap(), b"new image bytes");
		let restored = restore_user(&archives, &id, &s.0.join("profiles")).unwrap();
		assert_eq!(fs::read(restored).unwrap(), p.manifest);
		assert_eq!(fs::read(old).unwrap(), b"old image bytes");
	}
	#[test]
	fn icon_publication_failure_keeps_existing_assets() {
		let s = Sandbox::new();
		let old = store_icon(&s.0, b"existing").unwrap();
		let blocked = s.0.join("blocked");
		fs::write(&blocked, b"occupied").unwrap();
		assert!(store_icon(&blocked, b"new").is_err());
		assert_eq!(fs::read(old).unwrap(), b"existing");
	}
}
