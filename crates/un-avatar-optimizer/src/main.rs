#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::Value;

const GLB_MAGIC: u32 = 0x4654_6c67;
const GLB_VERSION_2: u32 = 2;
const JSON_CHUNK_TYPE: u32 = 0x4e4f_534a;
const BIN_CHUNK_TYPE: u32 = 0x004e_4942;

#[derive(Parser)]
#[command(name = "un-avatar-optimizer")]
#[command(about = "Experimental optimizer for .unavatar GLB files")]
struct Args {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Re-encode embedded GLB images to lossless WebP for size experiments.
	WebpLossless {
		input: PathBuf,
		output: PathBuf,
		/// Skip images that become larger after lossless WebP encoding.
		#[arg(long)]
		keep_larger_source: bool,
	},
}

#[derive(Debug)]
struct BufferViewBytes {
	bytes: Vec<u8>,
	target: Option<Value>,
}

#[derive(Default)]
struct OptimizeReport {
	images_seen: usize,
	images_converted: usize,
	images_kept: usize,
	images_failed: usize,
	original_image_bytes: usize,
	output_image_bytes: usize,
}

fn main() -> Result<(), String> {
	let args = Args::parse();
	match args.command {
		Command::WebpLossless {
			input,
			output,
			keep_larger_source,
		} => {
			let report = webp_lossless(&input, &output, keep_larger_source)?;
			println!("images seen: {}", report.images_seen);
			println!("images converted: {}", report.images_converted);
			println!("images kept: {}", report.images_kept);
			println!("images failed: {}", report.images_failed);
			println!("image bytes: {} -> {}", report.original_image_bytes, report.output_image_bytes);
			if report.original_image_bytes > 0 {
				let ratio = report.output_image_bytes as f64 / report.original_image_bytes as f64;
				println!("image byte ratio: {:.3}", ratio);
			}
		}
	}
	Ok(())
}

fn webp_lossless(input: &PathBuf, output: &PathBuf, keep_larger_source: bool) -> Result<OptimizeReport, String> {
	let bytes = fs::read(input).map_err(|e| format!("read {}: {e}", input.display()))?;
	let (mut json, bin) = read_glb(&bytes)?;
	let mut views = extract_buffer_views(&json, &bin)?;
	let mut report = OptimizeReport::default();

	let images = json
		.get_mut("images")
		.and_then(Value::as_array_mut)
		.ok_or_else(|| "glb has no images array".to_string())?;
	for image in images {
		report.images_seen += 1;
		let Some(view_index) = image.get("bufferView").and_then(Value::as_u64).map(|v| v as usize) else {
			continue;
		};
		if view_index >= views.len() {
			report.images_failed += 1;
			continue;
		}
		let source = &views[view_index].bytes;
		report.original_image_bytes += source.len();
		match encode_lossless_webp(source) {
			Ok(webp) if !keep_larger_source || webp.len() < source.len() => {
				report.output_image_bytes += webp.len();
				views[view_index].bytes = webp;
				if let Some(obj) = image.as_object_mut() {
					obj.insert("mimeType".to_string(), Value::String("image/webp".to_string()));
				}
				report.images_converted += 1;
			}
			Ok(_) => {
				report.output_image_bytes += source.len();
				report.images_kept += 1;
			}
			Err(_) => {
				report.output_image_bytes += source.len();
				report.images_failed += 1;
			}
		}
	}

	rebuild_glb(&mut json, &views, output)?;
	Ok(report)
}

fn read_glb(bytes: &[u8]) -> Result<(Value, Vec<u8>), String> {
	if bytes.len() < 12 {
		return Err("file is too small for GLB header".to_string());
	}
	let magic = read_u32(bytes, 0)?;
	let version = read_u32(bytes, 4)?;
	if magic != GLB_MAGIC || version != GLB_VERSION_2 {
		return Err("file is not GLB 2.0".to_string());
	}
	let mut offset = 12usize;
	let mut json = None;
	let mut bin = Vec::new();
	while offset + 8 <= bytes.len() {
		let length = read_u32(bytes, offset)? as usize;
		let chunk_type = read_u32(bytes, offset + 4)?;
		offset += 8;
		if offset + length > bytes.len() {
			return Err("GLB chunk exceeds file length".to_string());
		}
		let chunk = &bytes[offset..offset + length];
		match chunk_type {
			JSON_CHUNK_TYPE => {
				let text = String::from_utf8(chunk.iter().copied().take_while(|b| *b != 0).collect())
					.map_err(|e| format!("parse JSON chunk UTF-8: {e}"))?;
				json = Some(serde_json::from_str(&text).map_err(|e| format!("parse GLB JSON: {e}"))?);
			}
			BIN_CHUNK_TYPE => bin = chunk.to_vec(),
			_ => {}
		}
		offset += length;
	}
	Ok((json.ok_or_else(|| "GLB JSON chunk is missing".to_string())?, bin))
}

fn extract_buffer_views(json: &Value, bin: &[u8]) -> Result<Vec<BufferViewBytes>, String> {
	let array = json
		.get("bufferViews")
		.and_then(Value::as_array)
		.ok_or_else(|| "glb has no bufferViews array".to_string())?;
	let mut views = Vec::with_capacity(array.len());
	for view in array {
		let byte_offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
		let byte_length = view
			.get("byteLength")
			.and_then(Value::as_u64)
			.ok_or_else(|| "bufferView without byteLength".to_string())? as usize;
		if byte_offset + byte_length > bin.len() {
			return Err("bufferView exceeds BIN chunk".to_string());
		}
		views.push(BufferViewBytes {
			bytes: bin[byte_offset..byte_offset + byte_length].to_vec(),
			target: view.get("target").cloned(),
		});
	}
	Ok(views)
}

fn encode_lossless_webp(source: &[u8]) -> Result<Vec<u8>, String> {
	let image = image::load_from_memory(source).map_err(|e| format!("decode image: {e}"))?;
	let rgba = image.to_rgba8();
	let mut output = Vec::new();
	image::codecs::webp::WebPEncoder::new_lossless(&mut output)
		.encode(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
		.map_err(|e| format!("encode lossless WebP: {e}"))?;
	Ok(output)
}

fn rebuild_glb(json: &mut Value, views: &[BufferViewBytes], output: &PathBuf) -> Result<(), String> {
	let mut bin = Vec::new();
	let mut buffer_views = Vec::with_capacity(views.len());
	for view in views {
		align_to_4(&mut bin, 0);
		let byte_offset = bin.len();
		bin.extend_from_slice(&view.bytes);
		let mut obj = serde_json::Map::new();
		obj.insert("buffer".to_string(), Value::from(0));
		obj.insert("byteOffset".to_string(), Value::from(byte_offset as u64));
		obj.insert("byteLength".to_string(), Value::from(view.bytes.len() as u64));
		if let Some(target) = &view.target {
			obj.insert("target".to_string(), target.clone());
		}
		buffer_views.push(Value::Object(obj));
	}
	align_to_4(&mut bin, 0);
	json["bufferViews"] = Value::Array(buffer_views);
	if let Some(buffer) = json
		.get_mut("buffers")
		.and_then(Value::as_array_mut)
		.and_then(|buffers| buffers.get_mut(0))
	{
		if let Some(obj) = buffer.as_object_mut() {
			obj.insert("byteLength".to_string(), Value::from(bin.len() as u64));
		}
	}

	let mut json_bytes = serde_json::to_vec(json).map_err(|e| format!("serialize GLB JSON: {e}"))?;
	align_to_4(&mut json_bytes, b' ');
	let total_length = 12 + 8 + json_bytes.len() + 8 + bin.len();
	let mut out = Vec::with_capacity(total_length);
	write_u32(&mut out, GLB_MAGIC);
	write_u32(&mut out, GLB_VERSION_2);
	write_u32(&mut out, total_length as u32);
	write_u32(&mut out, json_bytes.len() as u32);
	write_u32(&mut out, JSON_CHUNK_TYPE);
	out.extend_from_slice(&json_bytes);
	write_u32(&mut out, bin.len() as u32);
	write_u32(&mut out, BIN_CHUNK_TYPE);
	out.extend_from_slice(&bin);
	fs::write(output, out).map_err(|e| format!("write {}: {e}", output.display()))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
	let slice = bytes.get(offset..offset + 4).ok_or_else(|| "unexpected end of file".to_string())?;
	Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
	out.extend_from_slice(&value.to_le_bytes());
}

fn align_to_4(bytes: &mut Vec<u8>, padding: u8) {
	while bytes.len() % 4 != 0 {
		bytes.push(padding);
	}
}
