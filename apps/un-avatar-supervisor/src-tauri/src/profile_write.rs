//! Replace complete profile contents without truncating the existing file first.
//! This does not provide conflict detection between independent editors.
use std::{fs, io::{self, Write}, path::Path};

pub(crate) fn revision(contents: &[u8]) -> String {
	use sha2::{Digest, Sha256};
	format!("{:x}", Sha256::digest(contents))
}

pub(crate) fn check_revision(contents: &[u8], expected: Option<&str>) -> Result<(), String> {
	if expected.is_some_and(|expected| expected != revision(contents)) {
		return Err("profile_conflict: saved profile changed; reload and review before retrying".into());
	}
	Ok(())
}

/// Caller holds the store lock. Check the complete order before any publication.
pub(crate) fn check_reorder(paths: &[(String, std::path::PathBuf)], expected: Option<&std::collections::BTreeMap<String,String>>) -> Result<(),String> {
    let ids=paths.iter().map(|(id,_)|id).collect::<std::collections::BTreeSet<_>>();
    if ids.len()!=paths.len(){return Err("duplicate profile in reorder".into());}
    if let Some(expected)=expected {
        if expected.len()!=ids.len()||expected.keys().any(|id|!ids.contains(id)){return Err("profile_conflict: reorder target set changed".into());}
        for (id,path) in paths {
            let bytes=fs::read(path).map_err(|e|format!("read {}: {e}",path.display()))?;
            check_revision(&bytes,expected.get(id).map(String::as_str))?;
        }
    }
    Ok(())
}

/// Keep the guard alive from read through publication/rename. The persistent
/// sidecar is directory-scoped so replacing/renaming the manifest cannot
/// invalidate the lock. Never remove it on unlock (waiters may hold the inode).
/// Only cooperating writers are serialized; this is not stale-editor detection.
pub(crate) fn lock_store(directory: &Path) -> io::Result<fs::File> {
    fs::create_dir_all(directory)?;
    lock_directory(&directory.join(".lifecycle"))
}

pub(crate) fn lock_directory(path: &Path) -> io::Result<fs::File> {
	let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
	let parent = fs::canonicalize(parent)?;
	let guard = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
		.open(parent.join(".un-avatar-profile-write.lock"))?;
	guard.lock()?;
	Ok(guard)
}

/// Publish a new profile only if the destination is still unused.
pub(crate) fn create(path: &Path, contents: &[u8]) -> io::Result<()> {
	let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
	let mut staged = tempfile::NamedTempFile::new_in(parent)?;
	staged.write_all(contents)?;
	staged.as_file().sync_all()?;
	staged.persist_noclobber(path).map_err(|error| error.error)?;
	Ok(())
}

pub(crate) fn replace(path: &Path, contents: &[u8]) -> io::Result<()> {
	// Preserve the prior write-through behavior for existing symbolic links.
	let resolved;
	let path = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
		resolved = fs::canonicalize(path)?;
		resolved.as_path()
	} else { path };
	let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
	let mut staged = tempfile::NamedTempFile::new_in(parent)?;
	staged.write_all(contents)?;
	match fs::metadata(path) {
		Ok(metadata) => {
			if metadata.permissions().readonly() {
				return Err(io::Error::new(io::ErrorKind::PermissionDenied, "profile is read-only"));
			}
			staged.as_file().set_permissions(metadata.permissions())?;
		}
		Err(error) if error.kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(error),
	}
	staged.as_file().sync_all()?;
	// Same-directory staging keeps replacement on one filesystem. On failure,
	// dropping PersistError also removes only our temporary file.
	staged.persist(path).map_err(|error| error.error)?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn reorder_checks_every_revision_before_any_write() {
		let root=tempfile::tempdir().unwrap();
		let paths:Vec<(String,std::path::PathBuf)>=vec![("a".into(),root.path().join("a.toml")),("b".into(),root.path().join("b.toml"))];
		for (_,path) in &paths{fs::write(path,b"original").unwrap();}
		let expected=paths.iter().map(|(id,_)|(id.clone(),revision(b"original"))).collect::<std::collections::BTreeMap<_,_>>();
		assert!(check_reorder(&paths,Some(&expected)).is_ok());
		fs::write(&paths[1].1,b"external edit").unwrap();
		assert!(check_reorder(&paths,Some(&expected)).unwrap_err().contains("profile_conflict"));
		assert_eq!(fs::read(&paths[0].1).unwrap(),b"original");
		assert_eq!(fs::read(&paths[1].1).unwrap(),b"external edit");
		assert!(check_reorder(&paths[..1],Some(&expected)).is_err());
		assert!(check_reorder(&[paths[0].clone(),paths[0].clone()],None).is_err());
	}

	#[test]
	#[ignore = "launched only by the cross-process lock test"]
	fn transaction_lock_process_worker() {
		let path = std::path::PathBuf::from(std::env::var_os("UNA_TEST_TRANSACTION_PATH").unwrap());
		for _ in 0..12 {
			let _guard = if std::env::var_os("UNA_TEST_STORE_LOCK").is_some() {
                lock_store(path.parent().unwrap()).unwrap()
            } else { lock_directory(&path).unwrap() };
			let value: u32 = fs::read_to_string(&path).unwrap().parse().unwrap();
			std::thread::sleep(std::time::Duration::from_millis(2));
			replace(&path, (value + 1).to_string().as_bytes()).unwrap();
		}
	}

	#[test]
	fn directory_transaction_serializes_independent_processes() {
		let root = tempfile::tempdir().unwrap();
		let target = root.path().join("counter.toml");
		fs::write(&target, "0").unwrap();
		let mut children: Vec<_> = (0..2).map(|index| {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "profile_write::tests::transaction_lock_process_worker", "--ignored"])
                .env("UNA_TEST_TRANSACTION_PATH", &target).env_remove("UNA_TEST_STORE_LOCK");
            if index == 0 { command.env("UNA_TEST_STORE_LOCK", "1"); }
            command.spawn().unwrap()
		}).collect();
		for child in &mut children { assert!(child.wait().unwrap().success()); }
		assert_eq!(fs::read_to_string(&target).unwrap(), "24");
	}

	#[test]
	fn stale_revision_rejects_before_replacement_and_new_revision_can_retry() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("profile.toml");
		create(&path, b"old").unwrap();
		let old = revision(&fs::read(&path).unwrap());
		{
			let _guard = lock_directory(&path).unwrap();
			replace(&path, b"newer external edit").unwrap();
		}
		let _guard = lock_directory(&path).unwrap();
		let current = fs::read(&path).unwrap();
		assert!(check_revision(&current, Some(&old)).unwrap_err().contains("profile_conflict"));
		assert_eq!(fs::read(&path).unwrap(), b"newer external edit");
		check_revision(&current, Some(&revision(&current))).unwrap();
		replace(&path, b"reviewed edit").unwrap();
		assert_eq!(fs::read(&path).unwrap(), b"reviewed edit");
	}

	#[test]
	fn creation_rejects_existing_destination_without_changing_either_profile() {
		let root = tempfile::tempdir().unwrap();
		let target = root.path().join("copy.toml");
		create(&target, b"first").unwrap();
		assert!(create(&target, b"second").is_err());
		assert_eq!(fs::read(&target).unwrap(), b"first");
		assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
	}

	#[test]
	fn concurrent_creators_publish_exactly_one_complete_profile() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("same.toml");
		let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
		let workers: Vec<_> = [b'A', b'B'].into_iter().map(|byte| {
			let path = path.clone();
			let barrier = barrier.clone();
			std::thread::spawn(move || {
				barrier.wait();
				create(&path, &vec![byte; 8192]).map(|()| byte)
			})
		}).collect();
		let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
		assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
		let winner = results.into_iter().find_map(Result::ok).unwrap();
		assert_eq!(fs::read(&path).unwrap(), vec![winner; 8192]);
		assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
	}

	#[test]
	fn replacement_and_creation_leave_other_profiles_unchanged() {
		let root = tempfile::tempdir().unwrap();
		let target = root.path().join("target.toml");
		let other = root.path().join("other.toml");
		fs::write(&target, "old profile with longer contents").unwrap();
		fs::write(&other, "other profile").unwrap();
		replace(&target, b"new").unwrap();
		replace(&root.path().join("created.toml"), b"created").unwrap();
		assert_eq!(fs::read(&target).unwrap(), b"new");
		assert_eq!(fs::read(&other).unwrap(), b"other profile");
		assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
	}

	#[cfg(windows)]
	#[test]
	fn locked_destination_failure_preserves_original_and_cleans_staging() {
		use std::os::windows::fs::OpenOptionsExt;
		let root = tempfile::tempdir().unwrap();
		let target = root.path().join("target.toml");
		fs::write(&target, "original").unwrap();
		let guard = fs::OpenOptions::new().read(true).share_mode(0).open(&target).unwrap();
		assert!(replace(&target, b"replacement").is_err());
		drop(guard);
		assert_eq!(fs::read(&target).unwrap(), b"original");
		assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
		replace(&target, b"retry").unwrap();
		assert_eq!(fs::read(&target).unwrap(), b"retry");
	}
}
