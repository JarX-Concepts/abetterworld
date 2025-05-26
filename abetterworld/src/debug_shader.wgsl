struct Uniform {
    mat: mat4x4<f32>,        // 4x4 matrix (64 bytes)
    offset: vec3<f32>,       // offset (12 bytes)
    padding: f32,            // padding (4 bytes) to align to 16 bytes
};

// Camera uniform buffer (view-projection matrix)
@group(0) @binding(0)
var<uniform> camera: Uniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // First apply both offsets to get relative positions
    let pos_world = input.position - camera.offset;

    // Finally transform by camera matrix to get clip space position
    output.position = camera.mat * vec4<f32>(pos_world, 1.0);
    return output;
}

// Fragment shader: solid color (e.g., yellow)
@fragment
fn main_fs() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 1.0, 0.2, 1.0); // yellowish
}