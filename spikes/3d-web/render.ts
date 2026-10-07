// Headless check for the spike's scenes, on Deno's WebGPU (wgpu and naga
// on Metal, the stack the app uses): validates a scene, writes PNG
// captures and measures GPU time per frame with timestamp queries.
//
//   deno run --unstable-webgpu -A spikes/3d-web/render.ts <scene|all> [options]
//     --size 1280x800     points; times --dpr for pixels (default 1280x800, dpr 2)
//     --dpr 2
//     --scale 1           render scale (0.5 renders a quarter of the pixels)
//     --quality 1         raymarch steps scale (0.5 low, 1 medium, 1.5 high)
//     --look dark|light
//     --mode backdrop|visualiser
//     --palette vivid|muted|mono
//     --loud 0.8          the music's loudness 0..1 for the fake bands
//     --time 30           the clock in seconds
//     --seed <video id>
//     --mp3 <file>        the song the world scene builds its land from
//                         (decoded with ffmpeg); --time is then the
//                         playback position
//     --out <dir>         write <scene>-<mode>-<look>-<palette>.png there
//     --bench 120         also render this many frames and report GPU ms
//     --matrix            captures for every look x mode x palette (with --out)

import { bakeLand, bindEntries, encodeDraws, packParams, PARAMS_FLOATS, SCENES, sceneDraws, sceneSource, songTexture } from "./scenes.js";
import { emptyMap, songMap } from "./songmap.js";
import { colourKept, seed as seedOf, TEST_PALETTES } from "./cover.js";

const here = new URL(".", import.meta.url);
const args = parse(Deno.args);
const which = args._[0] ?? "all";
const scenes = which === "all" ? SCENES : SCENES.filter((s) => s.id === which);
if (scenes.length === 0) {
  console.error(`unknown scene ${which}; one of ${SCENES.map((s) => s.id).join(", ")}, all`);
  Deno.exit(2);
}

const adapter = await navigator.gpu.requestAdapter({ powerPreference: "high-performance" });
if (!adapter) throw new Error("no WebGPU adapter");
const timing = adapter.features.has("timestamp-query");
const device = await adapter.requestDevice({ requiredFeatures: timing ? ["timestamp-query"] : [] });
const format = "bgra8unorm";

const [pw, ph] = String(args.size ?? "1280x800").split("x").map(Number);
const dpr = Number(args.dpr ?? 2);
const scale = Number(args.scale ?? 1);
const width = Math.max(1, Math.round(pw * dpr * scale));
const height = Math.max(1, Math.round(ph * dpr * scale));

const map = args.mp3 ? await decodeMap(String(args.mp3)) : emptyMap();
if (args.mp3) console.log(`song: ${map.duration.toFixed(0)} s, ${map.key}, ${map.biomeName}, ${JSON.stringify(map.stats)}`);
const songMapTexture = songTexture(device, map);
const sampler = device.createSampler({ magFilter: "linear", minFilter: "linear", mipmapFilter: "linear" });

// Messages count lines in the scene's own file: common.wgsl comes first.
const commonLines = (await Deno.readTextFile(new URL("shaders/common.wgsl", here))).split("\n").length;
let failed = false;
// Wall time per frame over the timed run, a cross-check on the timestamps.
let wallPerFrame = 0;
for (const scene of scenes) {
  let source: string;
  try {
    source = await sceneSource((f: string) => Deno.readTextFile(new URL(`shaders/${f}`, here)), scene);
  } catch {
    console.log(`${scene.id}: no shader file yet`);
    continue;
  }
  device.pushErrorScope("validation");
  const module = device.createShaderModule({ code: source });
  const info = await module.getCompilationInfo();
  const errors = info.messages.filter((m) => m.type === "error");
  for (const m of info.messages) {
    console.log(`${scene.id}: ${m.type} line ${m.lineNum - commonLines}: ${m.message}`);
  }
  const pipeline = await sceneDraws(device, module, scene, format);
  const scopeError = await device.popErrorScope();
  if (errors.length || scopeError) {
    if (scopeError) console.log(`${scene.id}: ${scopeError.message}`);
    failed = true;
    continue;
  }
  console.log(`${scene.id}: valid`);

  const uniform = device.createBuffer({ size: PARAMS_FLOATS * 4, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  // The world bakes its land once, with the song's uniforms in place.
  let land = null;
  if (scene.songMap) {
    device.queue.writeBuffer(uniform, 0, packParams(fakeState({ look: "dark", mode: "backdrop", pal: "vivid" }, 0)));
    const started = performance.now();
    land = bakeLand(device, module, uniform, songMapTexture, sampler, map);
    await device.queue.onSubmittedWorkDone();
    console.log(`${scene.id}: baked ${land.width}x${land.height} in ${(performance.now() - started).toFixed(1)} ms`);
  }
  for (const d of pipeline) d.bind = device.createBindGroup({ layout: d.layout, entries: bindEntries(scene, uniform, songMapTexture, sampler, land) });
  const bind = null;
  const target = device.createTexture({ size: [width, height], format, usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });

  const runs = args.matrix
    ? ["dark", "light"].flatMap((look) => ["backdrop", "visualiser"].flatMap((mode) => ["vivid", "muted", "mono"].map((pal) => ({ look, mode, pal }))))
    : [{ look: args.look ?? "dark", mode: args.mode ?? "backdrop", pal: args.palette ?? "vivid" }];

  for (const run of runs) {
    const state = fakeState(run, Number(args.time ?? 30));
    device.queue.writeBuffer(uniform, 0, packParams(state));
    if (args.out) {
      draw(pipeline, bind, target, null);
      const png = await encodePng(await readBack(target), width, height);
      await Deno.mkdir(args.out, { recursive: true });
      const path = `${args.out}/${scene.id}-${run.mode}-${run.look}-${run.pal}.png`;
      await Deno.writeFile(path, png);
      console.log(`${scene.id}: wrote ${path}`);
    }
  }

  if (args.bench) {
    const frames = Number(args.bench);
    const state = fakeState(runs[0], Number(args.time ?? 30));
    const times = await bench(pipeline, bind, target, uniform, state, frames);
    if (times.length) {
      times.sort((a, b) => a - b);
      const median = times[Math.floor(times.length / 2)];
      const p95 = times[Math.floor(times.length * 0.95)];
      console.log(`${scene.id}: ${width}x${height} quality ${args.quality ?? 1}: GPU median ${median.toFixed(2)} ms, p95 ${p95.toFixed(2)} ms (${frames} frames; back to back ${wallPerFrame.toFixed(2)} ms a frame)`);
    } else {
      console.log(`${scene.id}: no timestamp-query on this adapter`);
    }
  }
  uniform.destroy();
  target.destroy();
}
Deno.exit(failed ? 1 : 0);

// A plausible frame of music: a falling spectrum with some movement,
// scaled by --loud.
function fakeState(run: { look: string; mode: string; pal: string }, seconds: number) {
  const loud = Number(args.loud ?? 0.8);
  const bands = new Float32Array(32);
  for (let i = 0; i < 32; i++) {
    const shape = 0.95 - 0.45 * (i / 31) + 0.12 * Math.sin(i * 1.7 + seconds * 2.1);
    bands[i] = Math.min(Math.max(shape * loud, 0), 1);
  }
  const mean = (a: number, b: number) => bands.slice(a, b).reduce((s, v) => s + v, 0) / (b - a);
  const palette = TEST_PALETTES[run.pal as keyof typeof TEST_PALETTES];
  return {
    seconds,
    bass: mean(0, 4),
    kick: 0.2 * loud,
    level: mean(0, 32),
    width,
    height,
    light: run.look === "light",
    visualiser: run.mode === "visualiser",
    strength: Number(args.strength ?? 1),
    colourKept: colourKept(palette),
    quality: Number(args.quality ?? 1),
    env: [mean(0, 4), mean(8, 20), mean(20, 32), mean(0, 32)],
    motion: [seconds, seconds, 0.6 * loud, 0.5 * loud],
    seed: seedOf(String(args.seed ?? "dQw4w9WgXcQ")),
    clock: seconds,
    songTime: seconds,
    songLength: map.duration,
    biome: map.biome,
    mood: map.mood,
    palette,
    bands,
  };
}

// deno-lint-ignore no-explicit-any
function draw(pipeline: any[], bind: null, target: GPUTexture, query: GPUQuerySet | null, index = 0) {
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{ view: target.createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }],
    ...(query ? { timestampWrites: { querySet: query, beginningOfPassWriteIndex: index * 2, endOfPassWriteIndex: index * 2 + 1 } } : {}),
  });
  encodeDraws(pass, pipeline);
  pass.end();
  device.queue.submit([encoder.finish()]);
}

// deno-lint-ignore no-explicit-any
async function bench(pipeline: any[], bind: null, target: GPUTexture, uniform: GPUBuffer, state: ReturnType<typeof fakeState>, frames: number) {
  if (!timing) return [];
  // One frame per submit, waited for: Apple GPUs overlap back-to-back
  // passes, so batched timestamps span neighbouring frames too.
  const batch = 1;
  const query = device.createQuerySet({ type: "timestamp", count: batch * 2 });
  const resolve = device.createBuffer({ size: batch * 16, usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC });
  const read = device.createBuffer({ size: batch * 16, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
  const times: number[] = [];
  // Warm up the pipeline first.
  for (let i = 0; i < 10; i++) draw(pipeline, bind, target, null);
  await device.queue.onSubmittedWorkDone();
  const started = performance.now();
  for (let i = 0; i < frames; i++) draw(pipeline, bind, target, null);
  await device.queue.onSubmittedWorkDone();
  wallPerFrame = (performance.now() - started) / frames;
  for (let done = 0; done < frames; done += batch) {
    const n = Math.min(batch, frames - done);
    for (let i = 0; i < n; i++) {
      state.clock += 1 / 120;
      state.seconds += 1 / 120;
      state.songTime += 1 / 120;
      device.queue.writeBuffer(uniform, 0, packParams(state));
      draw(pipeline, bind, target, query, i);
    }
    const encoder = device.createCommandEncoder();
    encoder.resolveQuerySet(query, 0, n * 2, resolve, 0);
    encoder.copyBufferToBuffer(resolve, 0, read, 0, n * 16);
    device.queue.submit([encoder.finish()]);
    await read.mapAsync(GPUMapMode.READ);
    const stamps = new BigInt64Array(read.getMappedRange().slice(0));
    read.unmap();
    // Metal sometimes hands back stamps from different clock domains
    // across submits (negative or huge spans); drop those.
    for (let i = 0; i < n; i++) {
      const ms = Number(stamps[i * 2 + 1] - stamps[i * 2]) / 1e6;
      if (ms > 0 && ms < 1000) times.push(ms);
    }
  }
  query.destroy();
  resolve.destroy();
  read.destroy();
  return times;
}

async function readBack(texture: GPUTexture) {
  const row = Math.ceil((width * 4) / 256) * 256;
  const buffer = device.createBuffer({ size: row * height, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
  const encoder = device.createCommandEncoder();
  encoder.copyTextureToBuffer({ texture }, { buffer, bytesPerRow: row }, [width, height]);
  device.queue.submit([encoder.finish()]);
  await buffer.mapAsync(GPUMapMode.READ);
  const src = new Uint8Array(buffer.getMappedRange());
  // BGRA rows to RGB scanlines with PNG's filter byte.
  const out = new Uint8Array((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    const o = y * (width * 3 + 1);
    for (let x = 0; x < width; x++) {
      const i = y * row + x * 4;
      out[o + 1 + x * 3] = src[i + 2];
      out[o + 2 + x * 3] = src[i + 1];
      out[o + 3 + x * 3] = src[i];
    }
  }
  buffer.unmap();
  buffer.destroy();
  return out;
}

async function encodePng(scanlines: Uint8Array, w: number, h: number) {
  const zlib = new Uint8Array(await new Response(new Blob([scanlines]).stream().pipeThrough(new CompressionStream("deflate"))).arrayBuffer());
  const header = new Uint8Array(13);
  const view = new DataView(header.buffer);
  view.setUint32(0, w);
  view.setUint32(4, h);
  header.set([8, 2, 0, 0, 0], 8);
  const parts = [new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", zlib), chunk("IEND", new Uint8Array())];
  const total = parts.reduce((s, p) => s + p.length, 0);
  const png = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    png.set(p, at);
    at += p.length;
  }
  return png;
}

function chunk(type: string, data: Uint8Array) {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  out.set(new TextEncoder().encode(type), 4);
  out.set(data, 8);
  view.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

function crc32(bytes: Uint8Array) {
  let c = ~0;
  for (const b of bytes) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function parse(list: string[]) {
  const out: Record<string, string | boolean> & { _: string[] } = { _: [] };
  for (let i = 0; i < list.length; i++) {
    const a = list[i];
    if (a.startsWith("--")) {
      const next = list[i + 1];
      if (next === undefined || next.startsWith("--")) out[a.slice(2)] = true;
      else out[a.slice(2)] = list[++i];
    } else out._.push(a);
  }
  return out as Record<string, any> & { _: string[] };
}

// The song decoded to mono with ffmpeg, then mapped.
async function decodeMap(path: string) {
  const rate = 22050;
  const out = await new Deno.Command("ffmpeg", { args: ["-v", "error", "-i", path, "-ac", "1", "-ar", String(rate), "-f", "f32le", "-"], stdout: "piped" }).output();
  if (!out.success) throw new Error(`ffmpeg couldn't decode ${path}`);
  const bytes = out.stdout;
  const samples = new Float32Array(bytes.buffer, bytes.byteOffset, Math.floor(bytes.byteLength / 4));
  return songMap(samples, rate);
}
