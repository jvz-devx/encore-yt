// The browser side of the M30 spike: plays an mp3 through Web Audio, runs
// the app's analysis on it and draws the chosen scene with WebGPU every
// paced frame, timing the GPU with timestamp queries.

import { Analysis, BANDS } from "./analysis.js";
import { colourKept, palette as coverPalette, readTag, seed as seedOf, TEST_PALETTES } from "./cover.js";
import { bakeLand, bindEntries, encodeDraws, packParams, PARAMS_FLOATS, SCENES, sceneDraws, sceneSource, songTexture } from "./scenes.js";
import { emptyMap, songMap } from "./songmap.js";

const $ = (id) => document.getElementById(id);
const ui = Object.fromEntries(
  ["canvas", "stage", "panel", "audio", "file", "open", "play", "scene", "mode", "look", "palette", "seed", "size", "scale", "quality", "fps", "strength", "overlay", "capture", "bench", "stats", "error", "bench-out", "np-cover", "np-title", "np-artist"]
    .map((id) => [id.replace(/-(\w)/g, (_, c) => c.toUpperCase()), $(id)]),
);

const song = { palette: TEST_PALETTES.vivid, hasCover: false, name: "", map: emptyMap() };
let songMapTexture = null;
let sampler = null;
let audioContext = null;
let analysis = null;
let device, context, format;
let uniform, querySet, resolveBuffer;
const readBuffers = [];
const pipelines = new Map();
const params = new Float32Array(PARAMS_FLOATS);
let clock = 0;
let flowClock = 0;
let sparkClock = 0;
let last = performance.now();
let lastDrawn = 0;
let captureNext = false;
// Rolling timings over about a second.
const gpuTimes = [];
const cpuTimes = [];
const frameGaps = [];

async function init() {
  if (!navigator.gpu) return fail("This browser has no WebGPU.");
  const adapter = await navigator.gpu.requestAdapter({ powerPreference: "high-performance" });
  if (!adapter) return fail("No WebGPU adapter.");
  const timing = adapter.features.has("timestamp-query");
  device = await adapter.requestDevice({ requiredFeatures: timing ? ["timestamp-query"] : [] });
  device.lost.then((info) => fail(`GPU device lost: ${info.message}`));
  format = navigator.gpu.getPreferredCanvasFormat();
  context = ui.canvas.getContext("webgpu");
  context.configure({ device, format, alphaMode: "opaque" });
  uniform = device.createBuffer({ size: PARAMS_FLOATS * 4, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  if (timing) {
    querySet = device.createQuerySet({ type: "timestamp", count: 2 });
    resolveBuffer = device.createBuffer({ size: 16, usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC });
  }
  songMapTexture = songTexture(device, song.map);
  sampler = device.createSampler({ magFilter: "linear", minFilter: "linear", mipmapFilter: "linear" });
  for (const [i, s] of SCENES.entries()) {
    ui.scene.add(new Option(`${i + 1}. ${s.name} (${s.concept})`, s.id));
  }
  restore();
  wire();
  const url = new URLSearchParams(location.search).get("mp3");
  if (url) loadUrl(url);
  requestAnimationFrame(frame);
}

function fail(message) {
  ui.error.textContent = message;
}

async function pipeline(id) {
  if (pipelines.has(id)) return pipelines.get(id);
  const scene = SCENES.find((s) => s.id === id);
  const entry = { ready: false };
  pipelines.set(id, entry);
  try {
    const source = await sceneSource(async (f) => {
      const r = await fetch(`shaders/${f}`, { cache: "no-store" });
      if (!r.ok) throw new Error(`shaders/${f}: ${r.status}`);
      return r.text();
    }, scene);
    const module = device.createShaderModule({ code: source });
    const info = await module.getCompilationInfo();
    const errors = info.messages.filter((m) => m.type === "error");
    if (errors.length) throw new Error(errors.map((m) => `line ${m.lineNum}: ${m.message}`).join("\n"));
    entry.draws = await sceneDraws(device, module, scene, format);
    entry.scene = scene;
    entry.module = module;
    if (!scene.songMap) {
      for (const d of entry.draws) d.bind = device.createBindGroup({ layout: d.layout, entries: bindEntries(scene, uniform, songMapTexture, sampler) });
    }
    entry.ready = true;
    ui.error.textContent = "";
  } catch (e) {
    entry.error = `${scene.name}: ${e.message}`;
  }
  return entry;
}

// Canvas pixels: the stage's size in device pixels times the render
// scale; the browser stretches it back (bilinear), as a lower-resolution
// backdrop would be in the app.
function resize() {
  const dpr = window.devicePixelRatio || 1;
  const scale = Number(ui.scale.value);
  const rect = ui.stage.getBoundingClientRect();
  const w = Math.max(1, Math.round(rect.width * dpr * scale));
  const h = Math.max(1, Math.round(rect.height * dpr * scale));
  if (ui.canvas.width !== w || ui.canvas.height !== h) {
    ui.canvas.width = w;
    ui.canvas.height = h;
  }
}

function frame(now) {
  requestAnimationFrame(frame);
  const cap = Number(ui.fps.value);
  frameGaps.push(now - last);
  if (frameGaps.length > 240) frameGaps.shift();
  if (cap && now - lastDrawn < 1000 / cap - 1.5) {
    last = now;
    return;
  }
  const dt = Math.min((now - (lastDrawn || now)) / 1000, 0.1);
  lastDrawn = now;
  last = now;
  const started = performance.now();
  draw(dt);
  cpuTimes.push(performance.now() - started);
  if (cpuTimes.length > 120) cpuTimes.shift();
}

function draw(dt) {
  const entry = pipelines.get(ui.scene.value);
  if (!entry) {
    pipeline(ui.scene.value);
    return;
  }
  if (!entry.ready) {
    if (entry.error) fail(entry.error);
    return;
  }
  resize();
  const playing = !ui.audio.paused;
  analysis?.update(dt, playing);
  const env = analysis?.env ?? [0, 0, 0, 0];
  clock += dt * (0.85 + 0.3 * env[3]);
  // Clocks that run with the beat (0.6x quiet to about 2.8x on a loud
  // hit), integrated so the pace changes and nothing jumps.
  const drive = analysis?.drive ?? [0, 0];
  flowClock += dt * (0.6 + 2.2 * drive[0]);
  sparkClock += dt * (0.6 + 2.4 * drive[1]);
  const pal = ui.palette.value === "cover" ? song.palette : TEST_PALETTES[ui.palette.value];
  packParams({
    seconds: performance.now() / 1000,
    bass: analysis?.bass ?? 0,
    kick: analysis?.kick ?? 0,
    level: analysis?.level ?? 0,
    width: ui.canvas.width,
    height: ui.canvas.height,
    light: ui.look.value === "light",
    visualiser: ui.mode.value === "visualiser",
    strength: Number(ui.strength.value),
    colourKept: colourKept(pal),
    quality: Number(ui.quality.value),
    env,
    seed: seedOf(ui.seed.value || "spike"),
    clock,
    motion: [flowClock, sparkClock, drive[0], drive[1]],
    songTime: ui.audio.src ? ui.audio.currentTime : clock,
    songLength: song.map.duration,
    biome: song.map.biome,
    mood: song.map.mood,
    palette: pal,
    bands: analysis?.levels ?? new Float32Array(BANDS),
  }, params);
  device.queue.writeBuffer(uniform, 0, params);
  // The world bakes its land once per song, with this song's uniforms.
  if (entry.scene.songMap && entry.landFor !== song.map) {
    entry.land?.destroy();
    const started = performance.now();
    entry.land = bakeLand(device, entry.module, uniform, songMapTexture, sampler, song.map);
    entry.landFor = song.map;
    for (const d of entry.draws) d.bind = device.createBindGroup({ layout: d.layout, entries: bindEntries(entry.scene, uniform, songMapTexture, sampler, entry.land) });
    device.queue.onSubmittedWorkDone().then(() => console.log(`baked ${entry.land.width}×${entry.land.height} in ${Math.round(performance.now() - started)} ms`));
  }

  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{ view: context.getCurrentTexture().createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }],
    ...(querySet ? { timestampWrites: { querySet, beginningOfPassWriteIndex: 0, endOfPassWriteIndex: 1 } } : {}),
  });
  encodeDraws(pass, entry.draws);
  pass.end();
  let read = null;
  if (querySet) {
    read = readBuffers.pop() ?? device.createBuffer({ size: 16, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
    encoder.resolveQuerySet(querySet, 0, 2, resolveBuffer, 0);
    encoder.copyBufferToBuffer(resolveBuffer, 0, read, 0, 16);
  }
  device.queue.submit([encoder.finish()]);
  if (read) {
    read.mapAsync(GPUMapMode.READ).then(() => {
      const [a, b] = new BigInt64Array(read.getMappedRange());
      read.unmap();
      readBuffers.push(read);
      // Chrome quantises timestamps to 100 µs unless WebGPU developer
      // features are on; the median over many frames still holds.
      if (b > a) {
        gpuTimes.push(Number(b - a) / 1e6);
        if (gpuTimes.length > 120) gpuTimes.shift();
      }
    }, () => readBuffers.push(read));
  }
  if (captureNext) {
    captureNext = false;
    saveCapture();
  }
}

function saveCapture() {
  const name = `${ui.scene.value}-${ui.mode.value}-${ui.look.value}-${ui.palette.value}.png`;
  ui.canvas.toBlob((blob) => {
    if (!blob) return fail("Capture failed");
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  });
}

function median(values) {
  if (!values.length) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function percentile(values, p) {
  if (!values.length) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
}

setInterval(() => {
  const gap = median(frameGaps);
  const lines = [
    `display   ${(1000 / gap).toFixed(0)} Hz`,
    `target    ${ui.canvas.width}×${ui.canvas.height}`,
    `GPU       ${fmt(median(gpuTimes))} ms median, ${fmt(percentile(gpuTimes, 0.95))} p95`,
    `CPU       ${fmt(median(cpuTimes))} ms a frame`,
  ];
  if (song.map.duration) {
    lines.push(`song      ${song.map.key}, ${song.map.biomeName}, ${Math.round(song.map.duration)} s`);
  }
  if (analysis) {
    const e = analysis.env.map((v) => v.toFixed(2)).join(" ");
    lines.push(`env       ${e}`, `drive     ${analysis.drive.map((v) => v.toFixed(2)).join(" ")} (pace ${(0.6 + 2.2 * analysis.drive[0]).toFixed(1)}x)`, `bands     ${bars(analysis.levels)}`);
  }
  ui.stats.textContent = lines.join("\n");
}, 500);

function fmt(v) {
  return Number.isFinite(v) ? v.toFixed(2) : "n/a";
}

function bars(levels) {
  const steps = "▁▂▃▄▅▆▇█";
  return Array.from(levels, (l) => steps[Math.min(7, Math.round(l * 7))]).join("");
}

// Runs every render scale and quality for the scene for two seconds each,
// at the display's rate, and prints a Markdown table.
async function bench() {
  const saved = { scale: ui.scale.value, quality: ui.quality.value, fps: ui.fps.value };
  ui.fps.value = "0";
  const rows = [];
  for (const scale of ["0.5", "0.75", "1"]) {
    for (const quality of ["0.5", "1", "1.5"]) {
      ui.scale.value = scale;
      ui.quality.value = quality;
      ui.benchOut.textContent = `Bench: scale ${scale}, quality ${quality}…`;
      await new Promise((r) => setTimeout(r, 400));
      gpuTimes.length = 0;
      cpuTimes.length = 0;
      await new Promise((r) => setTimeout(r, 2000));
      rows.push(`| ${scale} | ${quality} | ${ui.canvas.width}×${ui.canvas.height} | ${fmt(median(gpuTimes))} | ${fmt(percentile(gpuTimes, 0.95))} | ${fmt(median(cpuTimes))} |`);
    }
  }
  ui.scale.value = saved.scale;
  ui.quality.value = saved.quality;
  ui.fps.value = saved.fps;
  const scene = SCENES.find((s) => s.id === ui.scene.value);
  const table = [
    `${scene.name}, ${ui.mode.value}, ${ui.look.value}, display ${(1000 / median(frameGaps)).toFixed(0)} Hz`,
    "| Scale | Quality | Pixels | GPU median ms | GPU p95 ms | CPU ms |",
    "|---|---|---|---|---|---|",
    ...rows,
  ].join("\n");
  ui.benchOut.textContent = table;
  console.log(table);
}

async function loadFile(file) {
  const buffer = await file.arrayBuffer();
  await loadSong(new Blob([buffer], { type: file.type || "audio/mpeg" }), buffer, file.name);
}

async function loadUrl(url) {
  const r = await fetch(url);
  if (!r.ok) return fail(`Couldn't load ${url}`);
  const buffer = await r.arrayBuffer();
  await loadSong(new Blob([buffer], { type: "audio/mpeg" }), buffer, url.split("/").pop());
}

async function loadSong(blob, buffer, name) {
  const tag = readTag(buffer);
  ui.audio.src = URL.createObjectURL(blob);
  ui.play.disabled = false;
  ui.npTitle.textContent = tag.title ?? name.replace(/\.[^.]+$/, "");
  ui.npArtist.textContent = tag.artist ?? "Unknown artist";
  ui.seed.value = tag.title && tag.artist ? `${tag.artist} - ${tag.title}` : name;
  song.hasCover = false;
  song.palette = TEST_PALETTES.vivid;
  ui.npCover.style.backgroundImage = "";
  if (tag.picture) {
    try {
      const bitmap = await createImageBitmap(tag.picture);
      song.palette = coverPalette(bitmap);
      song.hasCover = true;
      ui.npCover.style.backgroundImage = `url(${URL.createObjectURL(tag.picture)})`;
    } catch {
      // A picture the browser can't decode: keep the test palette.
    }
  }
  ui.palette.value = song.hasCover ? "cover" : "vivid";
  await mapSong(buffer);
}

// Decodes the whole track and bakes the world's song map from it.
async function mapSong(buffer) {
  ui.benchOut.textContent = "Mapping the song…";
  try {
    const decoder = new OfflineAudioContext(1, 1, 44100);
    const audio = await decoder.decodeAudioData(buffer.slice(0));
    const mono = new Float32Array(audio.length);
    for (let c = 0; c < audio.numberOfChannels; c++) {
      const data = audio.getChannelData(c);
      for (let i = 0; i < data.length; i++) mono[i] += data[i] / audio.numberOfChannels;
    }
    const started = performance.now();
    song.map = songMap(mono, audio.sampleRate);
    // The old texture may still be bound for a frame in flight; it is
    // small, so let it go with the garbage instead of destroying it.
    songMapTexture = songTexture(device, song.map);
    ui.benchOut.textContent = `Song mapped in ${Math.round(performance.now() - started)} ms: ${song.map.key}, ${song.map.biomeName}`;
  } catch (e) {
    ui.benchOut.textContent = `Couldn't map the song: ${e.message}`;
  }
}

async function togglePlay() {
  if (!ui.audio.src) return;
  if (!audioContext) {
    audioContext = new AudioContext();
    const source = audioContext.createMediaElementSource(ui.audio);
    source.connect(audioContext.destination);
    analysis = new Analysis(audioContext, source);
  }
  await audioContext.resume();
  if (ui.audio.paused) await ui.audio.play();
  else ui.audio.pause();
}

const SAVED = ["scene", "mode", "look", "size", "scale", "quality", "fps", "strength", "overlay"];

function save() {
  try {
    localStorage.setItem("spike-3d", JSON.stringify(Object.fromEntries(SAVED.map((k) => [k, ui[k].value]))));
  } catch {
    // No storage: settings just don't stick.
  }
}

function restore() {
  try {
    const saved = JSON.parse(localStorage.getItem("spike-3d") ?? "{}");
    for (const k of SAVED) if (saved[k] !== undefined) ui[k].value = saved[k];
  } catch {
    // Ignore unreadable storage.
  }
  applyLayout();
}

function applyLayout() {
  document.documentElement.dataset.look = ui.look.value;
  ui.stage.classList.toggle("fixed", ui.size.value === "fixed");
  ui.stage.classList.toggle("visualiser", ui.mode.value === "visualiser");
  ui.stage.classList.toggle("no-overlay", ui.overlay.value === "off");
}

function wire() {
  for (const k of SAVED) {
    ui[k].addEventListener("input", () => {
      applyLayout();
      save();
      if (k === "scene") ui.error.textContent = "";
    });
  }
  ui.open.onclick = () => ui.file.click();
  ui.file.onchange = () => ui.file.files[0] && loadFile(ui.file.files[0]);
  ui.play.onclick = togglePlay;
  ui.audio.onplay = () => (ui.play.textContent = "Pause");
  ui.audio.onpause = () => (ui.play.textContent = "Play");
  ui.capture.onclick = () => (captureNext = true);
  ui.bench.onclick = bench;
  addEventListener("dragover", (e) => {
    e.preventDefault();
    document.body.classList.add("dragging");
  });
  addEventListener("dragleave", () => document.body.classList.remove("dragging"));
  addEventListener("drop", (e) => {
    e.preventDefault();
    document.body.classList.remove("dragging");
    const file = e.dataTransfer.files[0];
    if (file) loadFile(file);
  });
  addEventListener("keydown", (e) => {
    if (e.target instanceof HTMLInputElement && e.target.type === "text") return;
    const cycle = (select) => {
      select.selectedIndex = (select.selectedIndex + 1) % select.options.length;
      select.dispatchEvent(new Event("input"));
    };
    if (e.key === " ") {
      e.preventDefault();
      togglePlay();
    } else if (e.key >= "1" && e.key <= String(SCENES.length)) {
      ui.scene.value = SCENES[Number(e.key) - 1].id;
      ui.scene.dispatchEvent(new Event("input"));
    } else if (e.key === "m") cycle(ui.mode);
    else if (e.key === "l") cycle(ui.look);
    else if (e.key === "h") ui.panel.classList.toggle("hidden");
    else if (e.key === "c") captureNext = true;
  });
}

init();
