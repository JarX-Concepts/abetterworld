struct Uniform {
    mat: mat4x4<f32>,        // 4x4 matrix (64 bytes)
    offset: vec3<f32>,       // offset (12 bytes)
    padding: f32,            // padding (4 bytes) to align to 16 bytes
};

// Node (model) uniform buffer (model matrix)
@group(0) @binding(0)
var<uniform> node: Uniform;

// Texture and sampler remain in group 2
@group(1) @binding(0)
var my_texture: texture_2d<f32>;
@group(1) @binding(1)
var my_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) texcoord0: vec2<f32>,
    @location(4) texcoord1: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) texcoord0: vec2<f32>,
    @location(3) texcoord1: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // Apply per-object offset to the vertex position
    let local_position = input.position + node.offset;

    // Transform by the full model-view matrix to get clip-space position
    let world_position = node.mat * vec4<f32>(input.position, 1.0);

    output.position = world_position;

    output.color = input.color;
    output.normal = input.normal;
    output.texcoord0 = input.texcoord0;
    output.texcoord1 = input.texcoord1;
    return output;
}

@fragment
fn fs_main(
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) texcoord0: vec2<f32>,
    @location(3) texcoord1: vec2<f32>,
) -> @location(0) vec4<f32> {
    let tex_color = textureSample(my_texture, my_sampler, texcoord0);
    return tex_color * color;
}