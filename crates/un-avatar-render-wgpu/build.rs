fn main() {
	#[cfg(windows)]
	{
		let mut resource = winresource::WindowsResource::new();
		resource.set_icon("../../assets/icons/un-avatar-renderer.ico");
		resource.compile().expect("compile Windows resources");
	}
}
