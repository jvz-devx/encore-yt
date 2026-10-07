// Shared by the 3D scenes (M30: scene_xmb.wgsl, scene_ridges.wgsl,
// scene_aurora.wgsl; scene.rs puts this file in front of each): the
// parameters, noise, the palette and the tone mapping. From the browser
// spike (spikes/3d-web/, docs/gpui/SPIKE-3D.md).
// Output is display (gamma) space, BGRA8, opaque.

struct Params {
    // x: seconds, y: bass 0..1, z: kick 0..1, w: level 0..1 (the app's Bands)
    audio: vec4<f32>,
    // xy: target size in pixels, z: light look (0/1), w: visualiser (0 backdrop, 1 visualiser)
    output: vec4<f32>,
    // x: strength (1 = default), y: how much of the cover's colour is kept
    // (the backdrop's colour_kept), z: quality (raymarch steps scale),
    // w: how much the scene follows the music (1 in the visualiser)
    tune: vec4<f32>,
    // Slow envelopes 0..1: x: bass (~0.6 s), y: mids, z: highs, w: level (~2 s)
    env: vec4<f32>,
    // The song's seed: four values 0..1 from the video id
    seed: vec4<f32>,
    // x: the scene's clock (seconds, a little faster while the music is
    // loud), yzw: unused
    clock: vec4<f32>,
    palette: array<vec4<f32>, 4>,
    // The 32 bands, four per vec4, low to high
    bands: array<vec4<f32>, 8>,
    // x: a clock that runs with the music's energy (0.6x quiet to ~2.8x
    // on a loud hit), y: one that runs with its highs, z, w: those fast
    // envelopes 0..1 (quick rise, slow fall)
    motion: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;

// Dark look: the brightest the backdrop gets (relative luminance), so
// text_muted keeps 4.5:1 on top of it (backdrop.wgsl).
const DARK_CAP: f32 = 0.045;
const DARK_SATURATION: f32 = 1.7;
// Light look: the luminance range the backdrop is pressed into.
const LIGHT_FLOOR: f32 = 0.645;
const LIGHT_TOP: f32 = 0.8;
const LIGHT_CHROMA: f32 = 0.9;
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
// The visualiser's chroma cap in OKLab: cover colours, no neon.
const CHROMA_CAP: f32 = 0.14;
const PI: f32 = 3.14159265;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle that covers the target.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var out: VertexOut;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

// Dave Hoskins' hashes: floats only (32-bit integer multiplies are slow
// on the UHD 630, NOTES-visuals.md "GPU budget"), steady over the few
// thousand units a flight covers.
fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn hash11(p: f32) -> f32 {
    var x = fract(p * 0.1031);
    x *= x + 33.33;
    x *= x + x;
    return fract(x);
}

// Value noise with its derivatives: x the value 0..1, yz d/dx and d/dy.
fn noised(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    let k1 = b - a;
    let k2 = c - a;
    let k4 = a - b - c + d;
    return vec3<f32>(a + k1 * u.x + k2 * u.y + k4 * u.x * u.y,
        du * (vec2<f32>(k1, k2) + k4 * u.yx));
}

fn band(i: i32) -> f32 {
    let c = clamp(i, 0, 31);
    return params.bands[c / 4][c % 4];
}

// The bands at a fractional position 0..1, low to high, interpolated.
fn band_at(x: f32) -> f32 {
    let at = clamp(x, 0.0, 1.0) * 31.0;
    let i = i32(floor(at));
    return mix(band(i), band(i + 1), fract(at));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

// A palette colour in linear light.
fn pal(i: i32) -> vec3<f32> {
    return to_linear(params.palette[i].rgb);
}

fn oklab(c: vec3<f32>) -> vec3<f32> {
    let lms = vec3<f32>(
        0.41222146 * c.r + 0.53633255 * c.g + 0.051445995 * c.b,
        0.2119035 * c.r + 0.6806995 * c.g + 0.10739696 * c.b,
        0.08830246 * c.r + 0.28171885 * c.g + 0.6299787 * c.b,
    );
    let l = pow(max(lms, vec3<f32>(0.0)), vec3<f32>(1.0 / 3.0));
    return vec3<f32>(
        0.21045426 * l.x + 0.7936178 * l.y - 0.004072047 * l.z,
        1.9779985 * l.x - 2.4285922 * l.y + 0.4505937 * l.z,
        0.025904037 * l.x + 0.78277177 * l.y - 0.80867577 * l.z,
    );
}

fn oklab_to_linear(lab: vec3<f32>) -> vec3<f32> {
    let l = lab.x + 0.39633778 * lab.y + 0.21580376 * lab.z;
    let m = lab.x - 0.105561346 * lab.y - 0.06385417 * lab.z;
    let s = lab.x - 0.08948418 * lab.y - 1.2914855 * lab.z;
    let lms = vec3<f32>(l * l * l, m * m * m, s * s * s);
    return vec3<f32>(
        4.0767417 * lms.x - 3.3077116 * lms.y + 0.23096994 * lms.z,
        -1.268438 * lms.x + 2.6097574 * lms.y - 0.34131938 * lms.z,
        -0.0041960863 * lms.x - 0.7034186 * lms.y + 1.7076147 * lms.z,
    );
}

// Linear colour with its OKLab chroma held under `cap`.
fn cap_chroma(c: vec3<f32>, cap: f32) -> vec3<f32> {
    let lab = oklab(c);
    let chroma = length(lab.yz);
    let scale = min(1.0, cap / max(chroma, 1e-5));
    return max(oklab_to_linear(vec3<f32>(lab.x, lab.yz * scale)), vec3<f32>(0.0));
}

// Dark look (backdrop.wgsl): luminance compressed under DARK_CAP, hue
// kept, a floor so it never goes flat black. Takes linear light.
fn tone_dark(lin: vec3<f32>) -> vec3<f32> {
    let y = max(dot(lin, LUMA), 1e-4);
    let saturated = max(mix(vec3<f32>(y), lin, DARK_SATURATION * params.tune.y),
        vec3<f32>(0.0));
    let target_y = 0.004 + (DARK_CAP - 0.004) * (1.0 - exp(-y * 9.0));
    return pow(saturated * (target_y / y), vec3<f32>(1.0 / 2.2));
}

// Light look (backdrop.wgsl): luminance lifted into LIGHT_FLOOR..LIGHT_TOP,
// the colour's offset from grey scaled as far as the gamut allows.
fn tone_light(lin: vec3<f32>) -> vec3<f32> {
    let y = max(dot(lin, LUMA), 1e-4);
    let target_y = mix(LIGHT_FLOOR, LIGHT_TOP, smoothstep(0.05, 0.6, y));
    let offset = lin - vec3<f32>(y);
    let up = max(max(offset.r, max(offset.g, offset.b)), 1e-4);
    let down = max(max(-offset.r, max(-offset.g, -offset.b)), 1e-4);
    let fit = min(target_y / y, min((1.0 - target_y) / up, target_y / down));
    let toned = vec3<f32>(target_y) + offset * fit * min(LIGHT_CHROMA * params.tune.y, 1.0);
    return pow(clamp(toned, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
}

// The visualiser: a soft filmic curve (no pure white), chroma capped,
// lifted to a pastel in the light look.
fn tone_visualiser(lin: vec3<f32>) -> vec3<f32> {
    let c = cap_chroma(lin, CHROMA_CAP * (0.4 + 0.6 * params.tune.y));
    // Narkowicz's ACES fit, scaled so 1.0 maps under 0.92.
    let x = c * 0.8;
    var mapped = clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14),
        vec3<f32>(0.0), vec3<f32>(0.92));
    if params.output.z > 0.5 {
        mapped = mix(vec3<f32>(0.85), mapped, 0.72) + vec3<f32>(0.03);
    }
    return pow(mapped, vec3<f32>(1.0 / 2.2));
}

// Linear scene colour to the display value the page shows, per mode and
// look, dithered so gradients don't band.
fn finish(lin: vec3<f32>, frag: vec2<f32>) -> vec4<f32> {
    var out: vec3<f32>;
    if params.output.w > 0.5 {
        out = tone_visualiser(lin);
    } else if params.output.z > 0.5 {
        out = tone_light(lin);
    } else {
        out = tone_dark(lin);
    }
    let dither = (hash21(floor(frag)) - 0.5) / 255.0;
    return vec4<f32>(out + vec3<f32>(dither), 1.0);
}

// Pixel to a camera ray direction: x right, y up, 0 at the centre, the
// short side -1..1.
fn screen(frag: vec2<f32>) -> vec2<f32> {
    let size = params.output.xy;
    let p = (frag - 0.5 * size) / (0.5 * min(size.x, size.y));
    return vec2<f32>(p.x, -p.y);
}

fn rotate2(a: f32) -> mat2x2<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(c, s, -s, c);
}
