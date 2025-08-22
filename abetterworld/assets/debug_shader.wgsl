struct Camera {
  viewProj: mat4x4<f32>,
};

// Camera uniform buffer (view-projection matrix)
@group(0) @binding(0) var<uniform> uCamera : Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // First apply both offsets to get relative positions
    let pos_world = input.position;

    // Finally transform by camera matrix to get clip space position
    output.position = uCamera.viewProj * vec4<f32>(pos_world, 1.0);
    output.color = input.color;
    
    return output;
}

@fragment
fn fs_main(
    @location(0) color: vec4<f32>
) -> @location(0) vec4<f32> {
    return color;
}