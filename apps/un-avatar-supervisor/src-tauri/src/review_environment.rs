//! Compile-time-only native UI review boundary. Ordinary builds ignore its environment.
use std::path::PathBuf;
#[cfg(any(test,feature="isolated-review"))]
use std::path::Path;
#[cfg(all(feature="isolated-review",not(debug_assertions)))]
compile_error!("isolated-review is a debug-only verification artifact and cannot be packaged as release");
#[cfg(feature="isolated-review")]
static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
pub const ENABLED: bool = cfg!(feature="isolated-review");
pub const RUNTIME_ENABLED: bool = cfg!(feature="isolated-runtime-review");
#[cfg(feature="isolated-runtime-review")]
static RENDERER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
#[cfg(feature="isolated-runtime-review")]
static ATTACHED_MANIFEST: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
pub fn initialize() -> Result<(), String> {
    #[cfg(feature="isolated-review")]
    {
        if std::env::args_os().skip(1).next().is_some() { return Err("Isolated review accepts no startup proxy arguments".into()); }
        let path=std::env::var_os("UN_AVATAR_REVIEW_ROOT").ok_or("UN_AVATAR_REVIEW_ROOT required")?;
        let root=validate_root(Path::new(&path))?;
        #[cfg(feature="isolated-runtime-review")]
        {
            use sha2::{Digest, Sha256};
            let renderer=std::env::var_os("UN_AVATAR_REVIEW_RENDERER").ok_or("Explicit review Renderer required")?;
            let renderer=PathBuf::from(renderer);
            if !renderer.is_absolute() || renderer.file_name().and_then(|n|n.to_str()) != Some("un-avatar-renderer.exe") {return Err("Invalid review Renderer path".into());}
            let renderer=renderer.canonicalize().map_err(|e|e.to_string())?;
            let expected=std::env::var("UN_AVATAR_REVIEW_RENDERER_SHA256").map_err(|_|"Review Renderer hash required")?;
            let bytes=std::fs::read(&renderer).map_err(|e|e.to_string())?;
            if format!("{:x}",Sha256::digest(bytes)) != expected {return Err("Review Renderer hash mismatch".into());}
            RENDERER.set(renderer).map_err(|_|"Review Renderer already initialized")?;
            if let Some(manifest) = std::env::var_os("UN_AVATAR_REVIEW_ATTACH_MANIFEST") {
                let manifest=PathBuf::from(manifest);
                validate_manifest_under(&root, &manifest)?;
                ATTACHED_MANIFEST.set(manifest.canonicalize().map_err(|e|e.to_string())?).map_err(|_|"Review attachment already initialized")?;
            }
        }
        ROOT.set(root).map_err(|_|"Review environment already initialized")?;
    }
    Ok(())
}
pub fn root() -> Option<PathBuf> {
    #[cfg(feature="isolated-review")]
    { return Some(ROOT.get().expect("Review environment must initialize before storage").clone()); }
    #[cfg(not(feature="isolated-review"))]
    { None }
}
#[cfg(any(test,feature="isolated-review"))]
fn validate_root(path:&Path)->Result<PathBuf,String>{
    if !path.is_absolute(){return Err("Review root must be absolute".into());}
    let root=path.canonicalize().map_err(|e|format!("Review root: {e}"))?;
    let marker=std::fs::read_to_string(root.join(".un-avatar-review")).map_err(|e|format!("Review marker: {e}"))?;
    if marker.trim()!="isolated-native-ui-v1" {return Err("Invalid review marker".into());}
    Ok(root)
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn review_root_requires_explicit_existing_marked_directory(){
  assert!(validate_root(Path::new("relative")).is_err());
  let root=std::env::temp_dir().join(format!("una-review-root-{}",std::process::id()));
  std::fs::create_dir_all(&root).unwrap();assert!(validate_root(&root).is_err());
  std::fs::write(root.join(".un-avatar-review"),"wrong").unwrap();assert!(validate_root(&root).is_err());
  std::fs::write(root.join(".un-avatar-review"),"isolated-native-ui-v1").unwrap();assert_eq!(validate_root(&root).unwrap(),root.canonicalize().unwrap());
  std::fs::remove_dir_all(root).unwrap();
 }
}

// Runtime review retains the UI review store and disables shell/taskbar integration.
pub fn renderer_path() -> Option<PathBuf> {
    #[cfg(feature="isolated-runtime-review")]
    { return Some(RENDERER.get().expect("Review environment initialized").clone()); }
    #[cfg(not(feature="isolated-runtime-review"))]
    { None }
}
pub fn validate_runtime_manifest(path: &std::path::Path) -> Result<(),String> {
    if !RUNTIME_ENABLED {return Ok(());}
    let root=root().ok_or("Review root missing")?;
    validate_manifest_under(&root,path)
}
fn validate_manifest_under(root:&std::path::Path,path:&std::path::Path)->Result<(),String>{
    let root=root.canonicalize().map_err(|e|e.to_string())?;
    let canonical=path.canonicalize().map_err(|e|e.to_string())?;
    if !canonical.starts_with(root.join("config")) || !canonical.is_file() {return Err("Renderer manifest must be a file inside isolated config".into());}
    Ok(())
}
#[cfg(test)]
mod runtime_tests {
    use super::*;
    #[test]
    fn runtime_manifest_cannot_escape_isolated_config() {
        let root=std::env::temp_dir().join(format!("una-runtime-boundary-{}",std::process::id()));
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("config-other")).unwrap();
        let inside=root.join("config/profile.toml");
        let outside=root.join("config-other/profile.toml");
        std::fs::write(&inside,"").unwrap();std::fs::write(&outside,"").unwrap();
        assert!(validate_manifest_under(&root,&inside).is_ok());
        assert!(validate_manifest_under(&root,&outside).is_err());
        assert!(validate_manifest_under(&root,&root.join("config")).is_err());
        assert!(validate_manifest_under(&root,&root.join("config/missing.toml")).is_err());
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
}


// Only the isolated runtime build may attach an explicitly validated test profile.
pub fn attached_manifest() -> Option<PathBuf> {
 #[cfg(feature="isolated-runtime-review")] { return ATTACHED_MANIFEST.get().cloned(); }
 #[cfg(not(feature="isolated-runtime-review"))] { None }
}
