use std::{
	fs,
	path::{Path, PathBuf},
};

#[derive(Clone)]
pub(crate) struct PersistentPipelineCache {
	cache: Option<wgpu::PipelineCache>,
	path: Option<PathBuf>,
}

impl PersistentPipelineCache {
	pub(crate) fn load(device: &wgpu::Device, adapter_info: &wgpu::AdapterInfo) -> Self {
		let Some(key) = wgpu::util::pipeline_cache_key(adapter_info) else {
			return Self { cache: None, path: None };
		};
		let Some(dir) = pipeline_cache_dir() else {
			return Self { cache: None, path: None };
		};
		let path = dir.join(format!("{key}-wgpu29-v1.upc"));
		let data = fs::read(&path).ok();
		if let Some(data) = data.as_ref() {
			eprintln!(
				"un-avatar-renderer: Vulkan pipeline cache load path={} bytes={}",
				path.display(),
				data.len()
			);
		}
		let cache = create_pipeline_cache(device, data.as_deref());
		Self {
			cache: Some(cache),
			path: Some(path),
		}
	}

	pub(crate) fn cache(&self) -> Option<&wgpu::PipelineCache> {
		self.cache.as_ref()
	}

	pub(crate) fn store(&self) {
		let (Some(cache), Some(path)) = (&self.cache, &self.path) else {
			return;
		};
		let Some(data) = cache.get_data() else {
			return;
		};
		if let Some(parent) = path.parent() {
			if fs::create_dir_all(parent).is_err() {
				return;
			}
		}
		let temp = cache_temp_path(path);
		let bytes = data.len();
		if fs::write(&temp, data).is_ok() {
			let _ = fs::rename(&temp, path);
			eprintln!(
				"un-avatar-renderer: Vulkan pipeline cache store path={} bytes={}",
				path.display(),
				bytes
			);
		}
	}
}

#[allow(unsafe_code)]
fn create_pipeline_cache(device: &wgpu::Device, data: Option<&[u8]>) -> wgpu::PipelineCache {
	// SAFETY: The only non-None data passed here is read from a file that this
	// module previously wrote from `PipelineCache::get_data` for the same
	// `wgpu::util::pipeline_cache_key` namespace. `fallback: true` asks wgpu to
	// discard invalid or stale data and create an empty cache instead.
	unsafe {
		device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
			label: Some("un-avatar-pipeline-cache"),
			data,
			fallback: true,
		})
	}
}

fn pipeline_cache_dir() -> Option<PathBuf> {
	#[cfg(windows)]
	{
		std::env::var_os("LOCALAPPDATA")
			.or_else(|| std::env::var_os("APPDATA"))
			.map(PathBuf::from)
			.map(|path| path.join("UN Avatar").join("pipeline-cache").join("v1"))
	}
	#[cfg(not(windows))]
	{
		std::env::var_os("XDG_CACHE_HOME")
			.map(PathBuf::from)
			.or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
			.map(|path| path.join("un-avatar").join("pipeline-cache").join("v1"))
	}
}

fn cache_temp_path(path: &Path) -> PathBuf {
	let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("pipeline-cache");
	path.with_file_name(format!("{file_name}.tmp"))
}
