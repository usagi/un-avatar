// CSFC (Compute Surface Fur Cards) generator.
//
// This is the first GPU-side skeleton: one deterministic card per source
// triangle. Density allocation, texture-driven length/mask/vector sampling,
// and animated physics will be layered onto this interface.

struct CsfcParams {
	source_triangle_count: u32,
	card_count: u32,
	max_generated_vertices: u32,
	max_generated_indices: u32,
	cards_per_triangle: u32,
	_seed: u32,
	randomize: f32,
	_pad1: u32,
	fur_length: f32,
	card_width: f32,
	root_offset: f32,
	gravity: f32,
	direction: vec4<f32>,
}

struct CsfcSourceVertex {
	position: vec4<f32>,
	normal: vec4<f32>,
	tangent: vec4<f32>,
	uv: vec4<f32>,
	joints: vec4<u32>,
	weights: vec4<f32>,
}

struct CsfcCardSource {
	indices: vec4<u32>,
	sample_index: u32,
	_pad0: u32,
	_pad1: u32,
	_pad2: u32,
}

struct CsfcGeneratedVertex {
	position_layer: vec4<f32>,
	normal_side: vec4<f32>,
	uv: vec2<f32>,
	alpha: f32,
	seed: u32,
	root_position: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: CsfcParams;
@group(0) @binding(1) var<storage, read> source_vertices: array<CsfcSourceVertex>;
@group(0) @binding(2) var<storage, read> card_sources: array<CsfcCardSource>;
@group(0) @binding(3) var<storage, read_write> generated_vertices: array<CsfcGeneratedVertex>;
@group(0) @binding(4) var<storage, read_write> generated_indices: array<u32>;
@group(0) @binding(5) var fur_vector_tex: texture_2d<f32>;
@group(0) @binding(6) var fur_length_mask_tex: texture_2d<f32>;
@group(0) @binding(7) var fur_noise_mask_tex: texture_2d<f32>;
@group(0) @binding(8) var fur_mask_tex: texture_2d<f32>;
@group(0) @binding(9) var fur_samp: sampler;

fn safe_normalize(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
	let len2 = dot(v, v);
	if len2 <= 0.0000001 {
		return fallback;
	}
	return v * inverseSqrt(len2);
}

fn make_card_side(normal: vec3<f32>, tangent: vec3<f32>) -> vec3<f32> {
	let tangent_side = tangent - normal * dot(tangent, normal);
	if dot(tangent_side, tangent_side) > 0.0000001 {
		return safe_normalize(tangent_side, vec3<f32>(1.0, 0.0, 0.0));
	}
	let axis = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(normal.y) < 0.999);
	return safe_normalize(cross(axis, normal), vec3<f32>(1.0, 0.0, 0.0));
}

fn unpack_fur_vector_map(texel: vec4<f32>, scale: f32) -> vec3<f32> {
	var n = texel.xyz * 2.0 - vec3<f32>(1.0);
	n.x = n.x * scale;
	n.y = n.y * scale;
	if dot(n, n) < 0.000001 {
		return vec3<f32>(0.0, 0.0, 1.0);
	}
	return safe_normalize(n, vec3<f32>(0.0, 0.0, 1.0));
}

fn interpolate3(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> vec3<f32> {
	return (a + b + c) * (1.0 / 3.0);
}

fn interpolate2(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> vec2<f32> {
	return (a + b + c) * (1.0 / 3.0);
}

fn hash_u32(x_in: u32) -> u32 {
	var x = x_in;
	x = x ^ (x >> 16u);
	x = x * 2146121005u;
	x = x ^ (x >> 15u);
	x = x * 2246822507u;
	x = x ^ (x >> 16u);
	return x;
}

fn unit_from_hash(seed: u32) -> f32 {
	let value = hash_u32(seed) >> 8u;
	return (f32(value) + 0.5) * (1.0 / 16777216.0);
}

fn radical_inverse_vdc(bits_in: u32) -> f32 {
	var bits = ((bits_in & 0x55555555u) << 1u) | ((bits_in & 0xAAAAAAAAu) >> 1u);
	bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
	bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
	bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
	bits = (bits << 16u) | (bits >> 16u);
	return f32(bits) * 2.3283064365386963e-10;
}

fn barycentric_sample(triangle_seed: u32, sample_index: u32) -> vec3<f32> {
	let seed = hash_u32(triangle_seed ^ sample_index * 2654435769u);
	let jitter = unit_from_hash(seed);
	let u = fract((f32(sample_index) + jitter) * 0.61803398875);
	let v = radical_inverse_vdc(sample_index ^ seed);
	let su = sqrt(u);
	return vec3<f32>(1.0 - su, su * (1.0 - v), su * v);
}

fn liltoon_fur_barycentric_point(point_index: u32, segment_count: u32) -> vec3<f32> {
	if point_index == 0u {
		return vec3<f32>(1.0, 0.0, 0.0);
	}
	if segment_count <= 3u {
		if point_index == 1u {
			return vec3<f32>(0.0, 1.0, 0.0);
		}
		if point_index == 2u {
			return vec3<f32>(0.0, 0.0, 1.0);
		}
		return vec3<f32>(1.0, 0.0, 0.0);
	}
	if point_index == 1u {
		return vec3<f32>(0.0, 0.5, 0.5);
	}
	if point_index == 2u {
		return vec3<f32>(0.0, 1.0, 0.0);
	}
	if point_index == 3u {
		return vec3<f32>(0.5, 0.0, 0.5);
	}
	if point_index == 4u {
		return vec3<f32>(0.0, 0.0, 1.0);
	}
	if point_index == 5u {
		return vec3<f32>(0.5, 0.5, 0.0);
	}
	if segment_count <= 6u {
		return vec3<f32>(1.0, 0.0, 0.0);
	}
	if point_index == 6u {
		return vec3<f32>(1.0 / 6.0, 4.0 / 6.0, 1.0 / 6.0);
	}
	if point_index == 7u {
		return vec3<f32>(0.0, 0.5, 0.5);
	}
	if point_index == 8u {
		return vec3<f32>(1.0 / 6.0, 1.0 / 6.0, 4.0 / 6.0);
	}
	if point_index == 9u {
		return vec3<f32>(0.5, 0.0, 0.5);
	}
	if point_index == 10u {
		return vec3<f32>(4.0 / 6.0, 1.0 / 6.0, 1.0 / 6.0);
	}
	if point_index == 11u {
		return vec3<f32>(0.5, 0.5, 0.0);
	}
	return vec3<f32>(1.0, 0.0, 0.0);
}

fn interpolate3b(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, bary: vec3<f32>) -> vec3<f32> {
	return a * bary.x + b * bary.y + c * bary.z;
}

fn interpolate2b(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, bary: vec3<f32>) -> vec2<f32> {
	return a * bary.x + b * bary.y + c * bary.z;
}

fn liltoon_vertex_noise(vertex_ids: vec3<u32>, weight: vec3<u32>) -> vec3<f32> {
	let seed = vertex_ids.x * weight.x + vertex_ids.y * weight.y + vertex_ids.z * weight.z;
	let n = vec3<u32>(seed) * vec3<u32>(1597334677u, 3812015801u, 2912667907u);
	return safe_normalize(vec3<f32>(n) * (2.0 / 4294967295.0) - vec3<f32>(1.0), vec3<f32>(0.0, 1.0, 0.0));
}

fn fur_vector_for_vertex(v: CsfcSourceVertex, random_dir: vec3<f32>) -> vec3<f32> {
	let normal = safe_normalize(v.normal.xyz, vec3<f32>(0.0, 1.0, 0.0));
	let tangent = v.tangent.xyz;
	let side_dir = make_card_side(normal, tangent);
	let bitangent = safe_normalize(cross(normal, side_dir), vec3<f32>(0.0, 0.0, 1.0));
	let vector_tex = unpack_fur_vector_map(textureSampleLevel(fur_vector_tex, fur_samp, v.uv.xy, 0.0), params.direction.w);
	let authored_vector = vec3<f32>(
		params.direction.x + vector_tex.x,
		params.direction.y + vector_tex.y,
		(params.direction.z + 0.001) * vector_tex.z,
	);
	var fur_vector = side_dir * authored_vector.x + bitangent * authored_vector.y + normal * authored_vector.z;
	fur_vector = fur_vector + random_dir * max(params.fur_length, 0.0) * clamp(params.randomize, 0.0, 1.0);
	fur_vector = safe_normalize(fur_vector, normal) * max(params.fur_length, 0.0);
	let length_mask = clamp(textureSampleLevel(fur_length_mask_tex, fur_samp, v.uv.xy, 0.0).r, 0.0, 1.0);
	fur_vector = fur_vector * length_mask;
	fur_vector.y = fur_vector.y - params.gravity * length(fur_vector);
	return fur_vector;
}

fn fur_tip_for_sample(
	v0: CsfcSourceVertex,
	v1: CsfcSourceVertex,
	v2: CsfcSourceVertex,
	bary: vec3<f32>,
	seed: u32,
	vertex_ids: vec3<u32>,
) -> CsfcGeneratedVertex {
	let normal = safe_normalize(interpolate3b(v0.normal.xyz, v1.normal.xyz, v2.normal.xyz, bary), vec3<f32>(0.0, 1.0, 0.0));
	let root = interpolate3b(v0.position.xyz, v1.position.xyz, v2.position.xyz, bary);
	let uv = interpolate2b(v0.uv.xy, v1.uv.xy, v2.uv.xy, bary);
	let fv0 = fur_vector_for_vertex(v0, liltoon_vertex_noise(vertex_ids, vec3<u32>(3u, 1u, 1u)));
	let fv1 = fur_vector_for_vertex(v1, liltoon_vertex_noise(vertex_ids, vec3<u32>(1u, 3u, 1u)));
	let fv2 = fur_vector_for_vertex(v2, liltoon_vertex_noise(vertex_ids, vec3<u32>(1u, 1u, 3u)));
	let fur_vector = interpolate3b(fv0, fv1, fv2, bary);
	return CsfcGeneratedVertex(
		vec4<f32>(root + fur_vector, 1.0),
		vec4<f32>(normal, 0.0),
		uv,
		1.0,
		seed,
		vec4<f32>(root, 1.0),
	);
}

fn fur_root_for_tip(tip: CsfcGeneratedVertex, v0: CsfcSourceVertex, v1: CsfcSourceVertex, v2: CsfcSourceVertex, bary: vec3<f32>, seed: u32) -> CsfcGeneratedVertex {
	let normal = safe_normalize(interpolate3b(v0.normal.xyz, v1.normal.xyz, v2.normal.xyz, bary), vec3<f32>(0.0, 1.0, 0.0));
	let root = interpolate3b(v0.position.xyz, v1.position.xyz, v2.position.xyz, bary);
	return CsfcGeneratedVertex(
		vec4<f32>(root, 0.0),
		vec4<f32>(normal, 0.0),
		tip.uv,
		tip.alpha,
		seed,
		vec4<f32>(root, 1.0),
	);
}

fn write_vertex(vertex_index: u32, center_position: vec3<f32>, layer: f32, normal: vec3<f32>, signed_half_width: f32, uv: vec2<f32>, alpha: f32, seed: u32) {
	generated_vertices[vertex_index] = CsfcGeneratedVertex(
		vec4<f32>(center_position, layer),
		vec4<f32>(normal, signed_half_width),
		uv,
		alpha,
		seed,
		vec4<f32>(center_position, 1.0),
	);
}

@compute @workgroup_size(64)
fn csfc_generate(@builtin(global_invocation_id) gid: vec3<u32>) {
	let card_index = gid.x;
	if card_index >= params.card_count {
		return;
	}
	let card_source = card_sources[card_index];
	let sample_index = card_source.sample_index;

	let vertex_base = card_index * 4u;
	let index_base = card_index * 6u;
	if vertex_base + 3u >= params.max_generated_vertices || index_base + 5u >= params.max_generated_indices {
		return;
	}

	let tri = card_source.indices;
	let v0 = source_vertices[tri.x];
	let v1 = source_vertices[tri.y];
	let v2 = source_vertices[tri.z];

	let segment_count = max(params.cards_per_triangle, 1u);
	let local_segment = sample_index % segment_count;
	let seed = (tri.x * 3u + tri.y * 5u + tri.z * 7u + local_segment * 11u) * 747796405u + 277803737u;
	let bary = liltoon_fur_barycentric_point(local_segment, segment_count);
	let next_seed = seed ^ 0x9E3779B9u;
	let next_bary = liltoon_fur_barycentric_point(local_segment + 1u, segment_count);
	let vertex_ids = tri.xyz;
	let tip0 = fur_tip_for_sample(v0, v1, v2, bary, seed, vertex_ids);
	let tip1 = fur_tip_for_sample(v0, v1, v2, next_bary, next_seed, vertex_ids);
	generated_vertices[vertex_base + 0u] = fur_root_for_tip(tip0, v0, v1, v2, bary, seed);
	generated_vertices[vertex_base + 1u] = tip0;
	generated_vertices[vertex_base + 2u] = fur_root_for_tip(tip1, v0, v1, v2, next_bary, next_seed);
	generated_vertices[vertex_base + 3u] = tip1;

	generated_indices[index_base + 0u] = vertex_base + 0u;
	generated_indices[index_base + 1u] = vertex_base + 1u;
	generated_indices[index_base + 2u] = vertex_base + 2u;
	generated_indices[index_base + 3u] = vertex_base + 2u;
	generated_indices[index_base + 4u] = vertex_base + 1u;
	generated_indices[index_base + 5u] = vertex_base + 3u;
}
