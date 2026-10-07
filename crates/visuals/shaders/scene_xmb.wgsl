// The PlayStation 3 XMB wave, ported from linkev's WebGL recreation
// (github.com/linkev/PlayStation-3-XMB, ps3xmbwave/: spline.js,
// spline-reverse.js, particles.js and their settings), which rebuilds the
// wave from a reverse-engineering pass over the PS3's spline.elf. Its
// licence:
//
// MIT License
//
// Copyright (c) 2025 Mart
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
// Three passes (scene.rs): the background gradient (fs_main), the wave as
// one 100x100 grid mesh whose rows fold over each other, white with a
// fresnel alpha from its screen-space normal (vs_wave, fs_wave), and
// additive point sparkles (vs_sparkle, fs_sparkle, 2000 instances).
//
// Differences from the reference: the wave's displacement texture (a
// B-spline through 28 control points per row, CPU-side) is evaluated
// analytically here, without its small synthetic kernel term (gain 0.04);
// the background takes its colour from the cover instead of a month; and
// the music drives the pace, never the wave's shape: the wave flows on a
// clock that runs with the music's energy and the sparkles on one that
// runs with its highs (params.motion), and the wave's glow and the
// sparkles' light lift with those envelopes. Behind text it follows as
// much of the music as the settings say, the visualiser all of it.

// The reference's settings (spline-settings.js, particles-settings.js).
const FLOW_SPEED: f32 = 0.18;
const TENSION: f32 = 0.12;
const DAMPING: f32 = 0.0001;
const LENGTH: f32 = 0.306001;
const SPACING: f32 = 407.658;
const TIME_STEP: f32 = 1.0;
const BAND_AMPLITUDE: f32 = 0.2;
const BAND_SECONDARY_FREQ: f32 = 7.0;
const BAND_SECONDARY_AMP: f32 = 0.025;
const TRAVEL_SPEED_1: f32 = 0.25;
const TRAVEL_AMP_1: f32 = 0.014;
const TRAVEL_SPEED_2: f32 = 0.15;
const TRAVEL_AMP_2: f32 = 0.008;
const PERTURBATION: f32 = 0.0998587;
const PERTURBATION_SCALE: f32 = 0.07;
const WAVE_COS_AMP: f32 = 0.09;
const WAVE_BIAS: f32 = -0.1;
const WAVE_HEIGHT_SCALE: f32 = 0.5;
const WAVE_SOFT_CLIP: f32 = 0.22;
const PIPELINE_BLEND: f32 = 0.45;
const FRESNEL_POWER: f32 = 4.0;
const FRESNEL_SCALE: f32 = 0.5;
const OPACITY: f32 = 0.7;
const BRIGHTNESS: f32 = 0.98;
const Z_DETAIL_SCALE: f32 = 0.08;
const FFD_SCALE_1: vec3<f32> = vec3<f32>(5.67726, 1.00077, 1.0);
const FFD_SCALE_2: vec3<f32> = vec3<f32>(2.82755, 1.27579, 2.88782);
const FFD_OFFSET: vec3<f32> = vec3<f32>(0.0, -0.469999, 0.0);
const FFD_Y_AMP: f32 = 0.05;
const FFD_Z_AMP: f32 = 0.06;
const GRID: u32 = 100u;
const SPARKLE_OPACITY: f32 = 0.75;
const SPARKLE_SIZE_BASE: f32 = 2.6;
const SPARKLE_SIZE_VAR: f32 = 1.5;

// How much of the music the scene follows: as Settings → Visuals → 3D
// says behind text (half by default), all of it in the visualiser.
fn xmb_follow() -> f32 {
    return params.tune.w;
}

// The wave's seconds: they run with the music's energy. A blend of two
// integrated clocks is itself smooth, so the backdrop's half measure
// never jumps either.
fn xmb_time() -> f32 {
    return mix(params.clock.x, params.motion.x, xmb_follow());
}

// The sparkles' seconds: they run with the highs.
fn xmb_sparkle_time() -> f32 {
    return mix(params.clock.x, params.motion.y, xmb_follow());
}

// How strong the light layers may be in each look: the dark backdrop's
// luminance cap leaves them little room, the visualiser all of it.
fn xmb_light_scale() -> f32 {
    if params.output.w > 0.5 {
        return 1.0;
    }
    return select(0.22, 0.55, params.output.z > 0.5) * params.tune.x;
}

// Background: a smoothstepped diagonal gradient, the cover's colour at the
// top to near black at the bottom (the PS3's night gradients), its angle
// from the song's seed within the range the PS3's presets use.
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let angle = radians(mix(70.0, 115.0, params.seed.x));
    let dir = vec2<f32>(cos(angle), sin(angle));
    // The gradient's span over the unit square along `dir` (y down).
    let corners = vec4<f32>(0.0, dir.x, dir.y, dir.x + dir.y);
    let t_min = min(min(corners.x, corners.y), min(corners.z, corners.w));
    let t_max = max(max(corners.x, corners.y), max(corners.z, corners.w));
    let u = clamp((dot(in.uv, dir) - t_min) / max(t_max - t_min, 1e-6), 0.0, 1.0);
    let g = u * u * (3.0 - 2.0 * u);
    // The cover's most colourful palette entry, lifted a little.
    var top = pal(0);
    var best = length(oklab(top).yz);
    for (var i = 1; i < 4; i++) {
        let c = length(oklab(pal(i)).yz);
        if c > best {
            best = c;
            top = pal(i);
        }
    }
    let start = top * 0.9;
    let end = top * 0.03;
    let lin = mix(start, end, g);
    if params.output.w > 0.5 {
        // The visualiser shows the gradient as the PS3 does, untouched by
        // the filmic curve.
        let dither = (hash21(floor(in.position.xy)) - 0.5) / 255.0;
        let out = scrimmed(pow(lin, vec3<f32>(1.0 / 2.2)), in.position.xy);
        return vec4<f32>(out + vec3<f32>(dither), 1.0);
    }
    return finish(lin, in.position.xy);
}

// The displacement texture's value at column `x` (0..1) and row `z`
// (-1..1): spline-reverse.js's per-row control points without the kernel.
fn xmb_displacement(x: f32, z: f32, flow: f32) -> f32 {
    let re_core = sin(flow * 0.25 + z * 1.7 + x * 6.2) * BAND_AMPLITUDE
        + cos(z * BAND_SECONDARY_FREQ + x * 4.8 + flow * 0.09) * BAND_SECONDARY_AMP;
    let legacy = sin((x * PI * 1.3 + z * 0.8) - flow * TRAVEL_SPEED_1) * TRAVEL_AMP_1 * TENSION
        + sin((x * PI * 2.8 - z * 1.2) + flow * TRAVEL_SPEED_2) * TRAVEL_AMP_2
        + PERTURBATION * PERTURBATION_SCALE
            * sin((x * (4.0 + LENGTH * 2.0) + z * 4.0 - flow * 0.6) * (SPACING * 0.01));
    return re_core * PIPELINE_BLEND + legacy * (1.0 - PIPELINE_BLEND);
}

struct WaveOut {
    @builtin(position) position: vec4<f32>,
    @location(0) pos: vec3<f32>,
};

// One vertex of the grid: six per cell, two triangles.
@vertex
fn vs_wave(@builtin(vertex_index) index: u32) -> WaveOut {
    let cell = index / 6u;
    let corner = index % 6u;
    let cx = cell % (GRID - 1u);
    let cy = cell / (GRID - 1u);
    var offset = array<vec2<u32>, 6>(
        vec2<u32>(0u, 0u), vec2<u32>(1u, 0u), vec2<u32>(0u, 1u),
        vec2<u32>(1u, 0u), vec2<u32>(1u, 1u), vec2<u32>(0u, 1u),
    );
    let g = vec2<u32>(cx, cy) + offset[corner];
    let a = vec2<f32>(g) / f32(GRID - 1u) * 2.0 - 1.0;
    let uv = (a + 1.0) * 0.5;

    let time = xmb_time();
    let flow = time * FLOW_SPEED * TIME_STEP;
    var p = vec3<f32>(a.x, xmb_displacement(uv.x, a.y, flow), a.y);
    let ffd1 = p * FFD_SCALE_1 + FFD_OFFSET;
    let ffd2 = p * FFD_SCALE_2 + FFD_OFFSET;
    p.y += sin(ffd1.x + time * FLOW_SPEED) * FFD_Y_AMP;
    p.z += cos(ffd2.z + time * FLOW_SPEED) * FFD_Z_AMP;
    var base_wave = cos(p.x * 2.0 - time * 0.5 * TIME_STEP) * WAVE_COS_AMP + WAVE_BIAS;
    base_wave *= 1.0 - DAMPING;
    base_wave += TENSION * sin(p.x * LENGTH + time * FLOW_SPEED * TIME_STEP * 0.25);
    let structured = PERTURBATION * PERTURBATION_SCALE * (
        sin((p.x * LENGTH * 6.0 + p.z * 0.5) * SPACING * 0.01 + time * FLOW_SPEED * TIME_STEP * 0.7) * 0.5
        + sin((p.x * LENGTH * 10.0 - p.z * 0.8) * SPACING * 0.005 - time * FLOW_SPEED * TIME_STEP * 0.35) * 0.25);
    var total = (base_wave + structured) * WAVE_HEIGHT_SCALE;
    total = WAVE_SOFT_CLIP * tanh(total / WAVE_SOFT_CLIP);
    p.y -= total;
    let x2 = fract(uv.x - time * FLOW_SPEED * 0.04 * TIME_STEP);
    p.z -= xmb_displacement(x2, a.y, flow) * Z_DETAIL_SCALE;

    var out: WaveOut;
    // The reference draws the mesh straight into clip space; WebGPU's
    // depth range is 0..1, so z goes to the middle (no depth test).
    out.position = vec4<f32>(p.x, p.y, 0.5, 1.0);
    out.pos = p;
    return out;
}

// White, as opaque as the surface is edge-on (its normal from the screen
// derivatives; WebGPU's y runs down where WebGL's runs up).
@fragment
fn fs_wave(in: WaveOut) -> @location(0) vec4<f32> {
    let n = normalize(cross(dpdx(in.pos), -dpdy(in.pos)));
    let f = FRESNEL_SCALE * pow(max(1.0 + dot(vec3<f32>(0.0, 0.0, -1.0), n), 0.0), FRESNEL_POWER);
    let glow = 0.85 + 0.45 * params.motion.z * xmb_follow();
    let alpha = clamp(f * OPACITY * BRIGHTNESS * glow, 0.0, 1.0) * xmb_light_scale()
        * xmb_under_text(in.position.xy);
    return vec4<f32>(xmb_ink(), alpha);
}

// The wave and sparkles fade to 40% under the light look's scrim, so the
// text over them stays calm (white over the scrim only lightens it).
fn xmb_under_text(frag: vec2<f32>) -> f32 {
    return 1.0 - 0.6 * scrim_mask(frag);
}

// The light's colour: white, or a pale tint of the cover where white
// would read grey-blue on a coloured ground (the dark backdrop).
fn xmb_ink() -> vec3<f32> {
    if params.output.w > 0.5 || params.output.z > 0.5 {
        return vec3<f32>(1.0);
    }
    return pow(mix(vec3<f32>(1.0), pal(0) / max(max(pal(0).r, max(pal(0).g, pal(0).b)), 1e-3), 0.35), vec3<f32>(1.0 / 2.2));
}

struct SparkleOut {
    @builtin(position) position: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) alpha: f32,
};

// One sparkle, a quad standing in for the reference's point sprite. Its
// seeds come from a hash of the instance (the reference uses random
// numbers): x and y uniform, z rarely large (a power of 8).
@vertex
fn vs_sparkle(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> SparkleOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[index];
    let i = f32(instance);
    let h = vec3<f32>(hash11(i * 3.0 + 1.0), hash11(i * 3.0 + 2.0), hash11(i * 3.0 + 3.0));
    let seed = vec3<f32>(h.x, h.y, pow(h.z, 8.0) + 0.1);

    let size = params.output.xy;
    let aspect = size.x / size.y;
    let ratio = clamp(aspect, 1.0, 2.0) * 0.375;
    let time = xmb_sparkle_time() * FLOW_SPEED;
    let x = fract(time * (seed.x - 0.5) / 15.0 + seed.y * 50.0) * 2.0 - 1.0;
    let y = sin(sign(seed.y) * time * (seed.y + 1.5) / 4.0 + seed.x * 100.0)
        / ((6.0 - seed.x * 4.0 * seed.y) / ratio);
    let twinkle = mix(
        sin(time * (seed.x + 0.5) * 12.0 + seed.y * 10.0),
        sin(time * (seed.y + 1.5) * 6.0 + seed.x * 4.0),
        y * 0.5 + 0.5) * seed.x + seed.y;
    // The reference's sizes are pixels at about 1080 lines; never smaller.
    let diameter = (seed.z * SPARKLE_SIZE_VAR + SPARKLE_SIZE_BASE) * max(size.y / 1080.0, 1.0);

    var out: SparkleOut;
    out.position = vec4<f32>(x + c.x * diameter / size.x, y + c.y * diameter / size.y, 0.5, 1.0);
    out.corner = c;
    out.alpha = twinkle * twinkle * (1.0 - fract(seed.x + time * 0.00285));
    return out;
}

@fragment
fn fs_sparkle(in: SparkleOut) -> @location(0) vec4<f32> {
    let d = dot(in.corner, in.corner);
    if d > 1.0 {
        discard;
    }
    let sparkle = (1.0 - d) * (1.0 - d);
    let lift = 0.7 + 1.1 * params.motion.w * xmb_follow();
    let a = in.alpha * SPARKLE_OPACITY * sparkle * lift * xmb_light_scale()
        * xmb_under_text(in.position.xy);
    return vec4<f32>(xmb_ink() * a, 1.0);
}
