// The scenes the spike shows, the uniform block they share
// (shaders/common.wgsl `Params`) and the song map the world scene reads.
// Used by the page and by render.ts.

import { halfFloats, LANES } from "./songmap.js";

export const SCENES = [
  { id: "world", name: "World per song", concept: "1 + 4", file: "world.wgsl", songMap: true },
  { id: "liquid", name: "Liquid form", concept: "3", file: "liquid.wgsl" },
  { id: "xmb", name: "XMB (PS3 port)", concept: "2", file: "xmb.wgsl", passes: [{ fs: "fs_main" }, { vs: "vs_wave", fs: "fs_wave", vertices: 99 * 99 * 6, blend: "alpha" }, { vs: "vs_sparkle", fs: "fs_sparkle", vertices: 6, instances: 2000, blend: "add" }] },
  { id: "ridges", name: "Spectrum ridges", concept: "4", file: "ridges.wgsl" },
  { id: "aurora", name: "Aurora", concept: "new", file: "aurora.wgsl" },
  { id: "glass", name: "Glass prism", concept: "new", file: "glass.wgsl" },
  { id: "nebula", name: "Nebula", concept: "new", file: "nebula.wgsl" },
];

// 19 vec4s: audio, output, tune, env, seed, clock, 4 palette, 8 bands,
// motion.
export const PARAMS_FLOATS = 76;

// `state`: { seconds, bass, kick, level, width, height, light, visualiser,
// strength, colourKept, quality, env: [4], seed: [4], clock,
// palette: [[r,g,b] x4], bands: Float32Array(32), and for the song map
// songTime, songLength, biome, mood, and motion: [4] }
export function packParams(state, out = new Float32Array(PARAMS_FLOATS)) {
  out.set([state.seconds, state.bass, state.kick, state.level], 0);
  out.set([state.width, state.height, state.light ? 1 : 0, state.visualiser ? 1 : 0], 4);
  out.set([state.strength, state.colourKept, state.quality, state.mood ?? 0.6], 8);
  out.set(state.env, 12);
  out.set(state.seed, 16);
  out.set([state.clock, state.songTime ?? state.clock, state.songLength ?? 0, state.biome ?? 0], 20);
  state.palette.forEach((c, i) => out.set([c[0], c[1], c[2], 1], 24 + i * 4));
  out.set(state.bands, 40);
  out.set(state.motion ?? [state.clock, state.clock, 0, 0], 72);
  return out;
}

const BLENDS = {
  alpha: { color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" }, alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha" } },
  add: { color: { srcFactor: "one", dstFactor: "one" }, alpha: { srcFactor: "one", dstFactor: "one" } },
};

// A scene's draws, in order: one full-screen triangle unless the scene
// lists passes (a mesh, instanced sprites) drawn over it, each with its
// own entry points, vertex and instance counts and blending.
export async function sceneDraws(device, module, scene, format) {
  const passes = scene.passes ?? [{ fs: "fs_main" }];
  return Promise.all(passes.map(async (p) => {
    const pipeline = await device.createRenderPipelineAsync({
      layout: "auto",
      vertex: { module, entryPoint: p.vs ?? "vs_main" },
      fragment: { module, entryPoint: p.fs, targets: [{ format, ...(p.blend ? { blend: BLENDS[p.blend] } : {}) }] },
      primitive: { topology: "triangle-list" },
    });
    return { pipeline, layout: pipeline.getBindGroupLayout(0), vertices: p.vertices ?? 3, instances: p.instances ?? 1, bind: null };
  }));
}

export function encodeDraws(pass, draws) {
  for (const d of draws) {
    pass.setPipeline(d.pipeline);
    pass.setBindGroup(0, d.bind);
    pass.draw(d.vertices, d.instances);
  }
}

export async function sceneSource(load, scene) {
  const [common, body] = await Promise.all([load("common.wgsl"), load(scene.file)]);
  return `${common}\n${body}`;
}

// The song map as a texture: LANES wide, a row per quarter second.
export function songTexture(device, map) {
  const texture = device.createTexture({
    size: [LANES, map.rows],
    format: "rgba16float",
    usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
  });
  device.queue.writeTexture({ texture }, halfFloats(map.data), { bytesPerRow: LANES * 8 }, [LANES, map.rows]);
  return texture;
}

// Bind group entries for a scene: the uniforms, plus the song map, its
// sampler and the baked land for the scenes that read them (the bake
// pass itself has no land yet).
export function bindEntries(scene, uniform, songMap, sampler, land = null) {
  const entries = [{ binding: 0, resource: { buffer: uniform } }];
  if (scene.songMap) {
    entries.push({ binding: 1, resource: songMap.createView() }, { binding: 2, resource: sampler });
    if (land) entries.push({ binding: 3, resource: land.createView() });
  }
  return entries;
}

// world.wgsl's bake: SPEED units a second of song, BAKE_HALF either side,
// BAKE_BEFORE and BAKE_AFTER around the song. Sixteen texels a unit,
// fewer for long songs so it stays under 8192 rows.
const SPEED = 1.5;
const BAKE_HALF = 32;
const BAKE_BEFORE = 16;
const BAKE_AFTER = 40;

export function bakeSize(map) {
  const length = map.duration * SPEED + BAKE_BEFORE + BAKE_AFTER;
  let perUnit = 16;
  while (length * perUnit > 8192) perUnit /= 2;
  return [Math.round(2 * BAKE_HALF * perUnit), Math.max(2, Math.ceil(length * perUnit))];
}

// Bakes the world's land for `map` into a new r16float texture. The
// uniforms must already hold this song's length and biome.
export function bakeLand(device, module, uniform, songMap, sampler, map) {
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module, entryPoint: "vs_main" },
    fragment: { module, entryPoint: "fs_bake", targets: [{ format: "r16float" }] },
    primitive: { topology: "triangle-list" },
  });
  const [width, height] = bakeSize(map);
  // Mips for the land's level of detail: distant ground reads coarser
  // levels, and the difference between levels is its ambient occlusion.
  const mipLevelCount = Math.min(7, Math.floor(Math.log2(Math.min(width, height))) + 1);
  const land = device.createTexture({ size: [width, height], format: "r16float", mipLevelCount, usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING });
  const bind = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: bindEntries({ songMap: true }, uniform, songMap, sampler) });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({ colorAttachments: [{ view: land.createView({ baseMipLevel: 0, mipLevelCount: 1 }), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 0] }] });
  pass.setPipeline(pipeline);
  pass.setBindGroup(0, bind);
  pass.draw(3);
  pass.end();
  // Each level the mean of four texels of the one above.
  const down = device.createShaderModule({ code: DOWNSAMPLE });
  const downPipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module: down, entryPoint: "vs_main" },
    fragment: { module: down, entryPoint: "fs_main", targets: [{ format: "r16float" }] },
    primitive: { topology: "triangle-list" },
  });
  for (let level = 1; level < mipLevelCount; level++) {
    const from = device.createBindGroup({ layout: downPipeline.getBindGroupLayout(0), entries: [{ binding: 0, resource: land.createView({ baseMipLevel: level - 1, mipLevelCount: 1 }) }] });
    const step = encoder.beginRenderPass({ colorAttachments: [{ view: land.createView({ baseMipLevel: level, mipLevelCount: 1 }), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 0] }] });
    step.setPipeline(downPipeline);
    step.setBindGroup(0, from);
    step.draw(3);
    step.end();
  }
  device.queue.submit([encoder.finish()]);
  return land;
}

const DOWNSAMPLE = `
@group(0) @binding(0) var above: texture_2d<f32>;
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(above)) - 1;
    let at = vec2<i32>(position.xy) * 2;
    let sum = textureLoad(above, min(at, size), 0).r + textureLoad(above, min(at + vec2<i32>(1, 0), size), 0).r
        + textureLoad(above, min(at + vec2<i32>(0, 1), size), 0).r + textureLoad(above, min(at + vec2<i32>(1, 1), size), 0).r;
    return vec4<f32>(sum * 0.25, 0.0, 0.0, 1.0);
}
`;
