struct Camera {
  viewProj: mat4x4<f32>,
};

struct Instance3x4 {
  r0: vec4<f32>,
  r1: vec4<f32>,
  r2: vec4<f32>,
};

struct InstanceBuffer {
  data: array<Instance3x4>,
};

@group(0) @binding(0) var<uniform> uCamera : Camera;
@group(0) @binding(1) var<storage, read> Instances : InstanceBuffer;

@group(1) @binding(0) var my_texture: texture_2d<f32>;
@group(1) @binding(1) var my_sampler: sampler;

struct VSIn {
  @location(0) position : vec3<f32>,
  @location(1) normal   : vec3<f32>,
  @location(2) color    : vec4<f32>,
  @location(3) tex0     : vec2<f32>,
  @location(4) tex1     : vec2<f32>,
  @builtin(instance_index) iid : u32,
};

struct VSOut {
  @builtin(position) pos : vec4<f32>,
  @location(0) color : vec4<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) tex0  : vec2<f32>,
  @location(3) tex1  : vec2<f32>,
};

@vertex
fn vs_main(in: VSIn) -> VSOut {
  var out: VSOut;

  let inst = Instances.data[in.iid];

  let v = vec4<f32>(in.position, 1.0);
  let world = vec3<f32>(
    dot(inst.r0, v),
    dot(inst.r1, v),
    dot(inst.r2, v),
  );

  out.pos = uCamera.viewProj * vec4<f32>(world, 1.0);

  let n = vec3<f32>(
    dot(inst.r0.xyz, in.normal),
    dot(inst.r1.xyz, in.normal),
    dot(inst.r2.xyz, in.normal),
  );
  out.normal = normalize(n);

  out.color = in.color;
  out.tex0  = in.tex0;
  out.tex1  = in.tex1;
  return out;
}

@fragment
fn fs_main(
  @location(0) color : vec4<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) tex0  : vec2<f32>,
  @location(3) tex1  : vec2<f32>,
) -> @location(0) vec4<f32> {
  let tex_color = textureSample(my_texture, my_sampler, tex0);
  return tex_color * color;
}