// The player bar strip, in one pass: the window's base colour with a slow
// glow of the cover's palette rising from the bottom edge and breathing
// with the beat; the seek bar (a glowing fill whose lower edge follows the
// song's loudness, embossed, with a playhead that pulses on the kick); and
// soft rings around the play button and the cover that swell on each beat.
// Positions are in output pixels. Output is BGRA8 in display (gamma) space.

struct Params {
    // x: seconds, y: breath 0..1, z: kick 0..1, w: glow amount 0..1
    clock: vec4<f32>,
    // xy: output size in pixels, z: light look (0/1), w: pixels per point
    output: vec4<f32>,
    // x: left, y: top of the slider box, z: right, w: progress 0..1
    seek: vec4<f32>,
    // x: length known (0/1), y: has a waveform (0/1), z: hover 0..1, w: on
    seek_state: vec4<f32>,
    // xy: the play button's centre, z: its radius, w: on (0/1)
    play: vec4<f32>,
    // The cover thumbnail: left, top, right, bottom
    cover: vec4<f32>,
    // x: the cover's corner radius, y: on (0/1), z: the halos' strength
    // (1 the Default look, 0 none)
    cover_state: vec4<f32>,
    // The most-replayed ridge over the seek bar: x: on (0/1), y: its
    // height in points where the heat is greatest
    ridge: vec4<f32>,
    // The window base as OKLab (xyz) and its relative luminance (w).
    base_lab: vec4<f32>,
    // Display-space colours: signal (the fill), ink (the playhead and the
    // unplayed track; w: the track's opacity).
    signal: vec4<f32>,
    ink: vec4<f32>,
    // The halos' colour, linear RGB (from the cover's accent and the look).
    halo: vec4<f32>,
    // Display space: the ridge ahead of the playhead (behind it is signal).
    muted: vec4<f32>,
    // The cover's four colours as OKLab a and b (xy). Colours that are
    // the same for every pixel are worked out once, by the app.
    palette_ab: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var wave: texture_2d<f32>;
@group(0) @binding(2) var heat: texture_2d<f32>;
@group(0) @binding(3) var wave_sampler: sampler;

// Dark look: the glow lifts OKLab lightness at most this much over the
// base (0.145), so text_faint keeps 4.5:1 (it needs a luminance <= 0.0095).
const DARK_LIFT: f32 = 0.05;
// The glow's largest OKLab chroma, per look.
const DARK_CHROMA: f32 = 0.085;
const LIGHT_CHROMA: f32 = 0.05;
// Light look: the glow lightens a little, never darkens (dark text on top).
const LIGHT_LIFT: f32 = 0.012;
// Dark look: the glow's largest relative luminance (text_faint needs 0.0091).
const DARK_CAP: f32 = 0.0085;
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

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

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn to_display(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, x <= vec3<f32>(0.0031308));
}

fn oklab_to_linear(lab: vec3<f32>) -> vec3<f32> {
    let l = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let c = vec3<f32>(l * l * l, m * m * m, s * s * s);
    return vec3<f32>(
        4.0767416621 * c.x - 3.3077115913 * c.y + 0.2309699292 * c.z,
        -1.2684380046 * c.x + 2.6097574011 * c.y - 0.3413193965 * c.z,
        -0.0041960863 * c.x - 0.7034186147 * c.y + 1.7076147010 * c.z,
    );
}

// `ab` with its length capped at `cap`.
fn cap_chroma(ab: vec2<f32>, cap: f32) -> vec2<f32> {
    let c = length(ab);
    return select(ab, ab * (cap / c), c > cap);
}

// The base colour with the palette's glow, as linear RGB. Four soft blobs,
// one per palette colour, drift along the bar; the glow is nothing at the
// top edge (it meets the sidebar there) and fullest at the bottom.
fn glow(p: vec2<f32>) -> vec3<f32> {
    let size = params.output.xy;
    let t = params.clock.x;
    let breath = params.clock.y;
    let light = params.output.z > 0.5;
    let u = p.x / size.x;
    let v = p.y / size.y;
    let warp = noise(vec2<f32>(u * 3.0 + t * 0.045, v * 1.3 - t * 0.03)) - 0.5;
    let uw = u + warp * 0.09;
    var ab = vec2<f32>(0.0);
    var weight = 0.0;
    for (var i = 0; i < 4; i++) {
        let fi = f32(i);
        // Anchored at 0.06, 0.35, 0.64 and 0.92 of the bar (no array: an
        // indexed local array goes through memory on some GPUs).
        let anchor = 0.06 + 0.29 * fi - 0.01 * step(2.5, fi);
        let centre = anchor + 0.06 * sin(t * (0.09 + 0.025 * fi) + fi * 1.7);
        let width = 0.17 + 0.03 * sin(t * 0.07 + fi * 2.3);
        let d = (uw - centre) / width;
        let w = exp(-d * d);
        ab += params.palette_ab[i].xy * w;
        weight += w;
    }
    ab /= max(weight, 1e-3);
    let rise = smoothstep(0.02, 0.92 - 0.12 * breath, v);
    let amount = min(weight, 1.0) * rise * rise * (0.72 + 0.28 * breath) * params.clock.w;
    let base = params.base_lab.xyz;
    var lab = base;
    if light {
        lab.x = min(base.x + LIGHT_LIFT * amount, 0.99);
        lab = vec3<f32>(lab.x, base.yz + cap_chroma(ab * 0.8, LIGHT_CHROMA) * amount);
    } else {
        lab.x = base.x + DARK_LIFT * amount;
        lab = vec3<f32>(lab.x, base.yz + cap_chroma(ab, DARK_CHROMA) * amount);
    }
    // Chroma moves luminance a little at a given lightness (blue darkens):
    // one correction keeps the light look at least as light as the base
    // and the dark look under the cap.
    let y_base = params.base_lab.w;
    for (var i = 0; i < 2; i++) {
        let y = dot(oklab_to_linear(lab), LUMA);
        let goal = select(min(y, DARK_CAP), max(y, y_base), light);
        if y > 1e-5 && goal != y {
            lab.x *= pow(goal / y, 1.0 / 3.0);
        }
    }
    // Near white there is little room for colour: give up chroma, not
    // lightness, until it fits sRGB.
    var rgb = oklab_to_linear(lab);
    for (var i = 0; i < 6; i++) {
        if max(rgb.r, max(rgb.g, rgb.b)) <= 1.0 && min(rgb.r, min(rgb.g, rgb.b)) >= 0.0 {
            break;
        }
        lab = vec3<f32>(lab.x, lab.yz * 0.7);
        rgb = oklab_to_linear(lab);
    }
    return rgb;
}

fn round_rect(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, r: f32) -> f32 {
    let centre = (lo + hi) * 0.5;
    let half = (hi - lo) * 0.5;
    let q = abs(p - centre) - half + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// A soft ring `gap` outside an edge at signed distance `d`, swelling with
// the kick.
fn ring(d: f32, gap: f32, kick: f32, s: f32) -> f32 {
    let at = (gap + 6.0 * kick) * s;
    let width = 2.4 * s;
    let x = d - at;
    return exp(-x * x / (2.0 * width * width)) * (0.1 + 0.55 * kick) * step(0.0, d);
}

fn halos(p: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let s = params.output.w;
    let kick = params.clock.z;
    let breath = params.clock.y;
    let light = params.output.z > 0.5;
    let tone = params.halo.rgb;
    let strength = select(1.0, 0.95, light) * params.cover_state.z;
    var out = color;
    // Beyond 60 points the spill and the ring are under half a step of 8-bit
    // colour: those pixels skip the work.
    let reach = 60.0 * s;
    if params.play.w > 0.5 && distance(p, params.play.xy) - params.play.z < reach {
        let d = distance(p, params.play.xy) - params.play.z;
        let spill = exp(-max(d, 0.0) / (9.0 * s)) * 0.16 * breath * step(0.0, d);
        out = mix(out, tone, clamp((ring(d, 2.0, kick, s) + spill) * strength, 0.0, 0.8));
    }
    let near_cover = all(p > params.cover.xy - reach) && all(p < params.cover.zw + reach);
    if params.cover_state.y > 0.5 && near_cover {
        let d = round_rect(p, params.cover.xy, params.cover.zw, params.cover_state.x);
        let spill = exp(-max(d, 0.0) / (12.0 * s)) * 0.14 * breath * step(0.0, d);
        out = mix(out, tone, clamp((ring(d, 2.0, kick, s) + spill) * strength, 0.0, 0.8));
    }
    return out;
}

fn wave_at(u: f32) -> f32 {
    if params.seek_state.y < 0.5 {
        return 0.0;
    }
    return textureSampleLevel(wave, wave_sampler, vec2<f32>(clamp(u, 0.0, 1.0), 0.5), 0.0).r;
}

// The seek bar over `color`. The kit slider's box is 24 points tall with a
// 6 point track in the middle; the fill keeps the track's top edge (the
// most-replayed ridge rises from it) and hangs below it as deep as the song
// is loud there.
fn seek_bar(p: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let s = params.output.w;
    let kick = params.clock.z;
    let light = params.output.z > 0.5;
    let x0 = params.seek.x;
    let x1 = params.seek.z;
    let len = max(x1 - x0, 1.0);
    let yc = params.seek.y + 12.0 * s;
    let top = yc - 3.0 * s;
    let u = (p.x - x0) / len;
    let known = params.seek_state.x > 0.5;
    let w = wave_at(u) * select(0.0, 1.0, known);
    let bottom = yc + 3.0 * s + w * 6.0 * s;
    let r = 3.0 * s;

    // Coverage of the rounded silhouette.
    let cx = clamp(p.x, x0 + r, x1 - r);
    let cy = clamp(p.y, top + r, max(bottom - r, top + r));
    let corner = clamp(r - length(p - vec2<f32>(cx, cy)) + 0.5, 0.0, 1.0);
    let rows = clamp(p.y - top + 0.5, 0.0, 1.0) * clamp(bottom - p.y + 0.5, 0.0, 1.0);
    let cover = min(corner, rows);

    let head = x0 + clamp(params.seek.w, 0.0, 1.0) * len;
    let played = select(0.0, clamp(head - p.x + 0.5, 0.0, 1.0), known);
    let depth = clamp((p.y - top) / max(bottom - top, 1.0), 0.0, 1.0);
    let rim = clamp(1.0 - (p.y - top) / (1.3 * s), 0.0, 1.0);
    let lower = clamp(1.0 - (bottom - p.y) / (1.6 * s), 0.0, 1.0);

    let signal = to_linear(params.signal.rgb);
    let ink = to_linear(params.ink.rgb);
    var out = color;

    // Bloom around the played part, under the bar.
    let dx = max(p.x - head, 0.0) + max(x0 - p.x, 0.0);
    let dy = max(top - p.y, 0.0) + max(p.y - bottom, 0.0);
    let sigma = 4.5 * s;
    let bloom = exp(-(dx * dx + dy * dy) / (2.0 * sigma * sigma)) * select(0.0, 1.0, known);
    let bloom_tone = select(signal, mix(signal, vec3<f32>(1.0), 0.45), light);
    out = mix(out, bloom_tone, bloom * (1.0 - cover) * select(0.3, 0.2, light) * (0.8 + 0.5 * kick));

    // The unplayed track: ink, faint, a little stronger in the waveform.
    let track_alpha = params.ink.w * (1.0 + 0.6 * depth) + 0.12 * rim;
    out = mix(out, ink, cover * (1.0 - played) * clamp(track_alpha, 0.0, 1.0));

    // The played fill: signal, lit along its top edge and shaded towards
    // the loudness edge, so the waveform reads as relief.
    var fill = signal * (1.12 - 0.42 * depth) * (1.0 - 0.3 * lower);
    fill = mix(fill, vec3<f32>(1.0), rim * select(0.38, 0.22, light));
    out = mix(out, fill, cover * played);

    if known {
        // The playhead: an ink disc that swells on the kick, with a signal
        // ring pulsing out of it.
        let d = distance(p, vec2<f32>(head, yc));
        let radius = (5.5 + 1.4 * kick + 1.2 * params.seek_state.z) * s;
        let pulse = d - radius - (1.5 + 4.0 * kick) * s;
        let ring_width = 1.6 * s;
        let pulse_alpha = exp(-pulse * pulse / (2.0 * ring_width * ring_width))
            * (0.12 + 0.6 * kick) * step(radius, d);
        out = mix(out, select(signal, bloom_tone, light), clamp(pulse_alpha, 0.0, 0.85));
        let shadow = clamp(radius + 1.5 * s - d, 0.0, 1.0) * select(0.35, 0.12, light);
        out = mix(out, vec3<f32>(0.0), shadow);
        out = mix(out, ink, clamp(radius - d + 0.5, 0.0, 1.0));
    }
    return out;
}

fn heat_at(u: f32) -> f32 {
    return textureSampleLevel(heat, wave_sampler, vec2<f32>(clamp(u, 0.0, 1.0), 0.5), 0.0).r;
}

// The most-replayed ridge (YouTube's replay heat) as a low hill rising
// from the track's top edge: signal behind the playhead, muted ahead, a
// one-point edge along its top.
fn ridge(p: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let s = params.output.w;
    let x0 = params.seek.x;
    let x1 = params.seek.z;
    if p.x < x0 || p.x > x1 {
        return color;
    }
    let len = max(x1 - x0, 1.0);
    let base = params.seek.y + 9.0 * s;
    let height = params.ridge.y * s;
    let u = (p.x - x0) / len;
    let top = base - height * heat_at(u);
    // The edge's distance across its slope, for an even line width.
    let slope = (heat_at(u + 1.0 / len) - heat_at(u - 1.0 / len)) * 0.5 * height;
    let across = abs(p.y - top) / sqrt(1.0 + slope * slope);
    let fill = clamp(p.y - top + 0.5, 0.0, 1.0) * clamp(base - p.y + 0.5, 0.0, 1.0);
    let edge = clamp(0.5 * s + 0.5 - across, 0.0, 1.0) * step(p.y, base + 0.5);
    let head = x0 + clamp(params.seek.w, 0.0, 1.0) * len;
    let played = select(0.0, clamp(head - p.x + 0.5, 0.0, 1.0), params.seek_state.x > 0.5);
    // Blended in display space, as GPUI blends the ridge it draws without
    // effects.
    let signal = params.signal.rgb;
    let muted = params.muted.rgb;
    var out = mix(to_display(color), muted, fill * 0.16 * (1.0 - played));
    out = mix(out, signal, fill * 0.42 * played);
    out = mix(out, muted, edge * 0.5 * (1.0 - played));
    return to_linear(mix(out, signal, edge * played));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let p = in.uv * params.output.xy;
    var color = glow(p);
    if params.cover_state.z > 0.0 {
        color = halos(p, color);
    }
    // The seek bar's bloom and playhead ring fade out within 20 points of
    // its track, and the ridge rises 11 above it: other pixels skip them.
    let s = params.output.w;
    let near_seek = p.x > params.seek.x - 20.0 * s && p.x < params.seek.z + 20.0 * s
        && p.y > params.seek.y - 8.0 * s && p.y < params.seek.y + 41.0 * s;
    if params.seek_state.w > 0.5 && near_seek {
        if params.ridge.x > 0.5 {
            color = ridge(p, color);
        }
        color = seek_bar(p, color);
    }
    let display = to_display(color) + (hash(p) - 0.5) / 255.0;
    // BGRA target: the swizzle happens on write.
    return vec4<f32>(display, 1.0);
}
