// The Now Playing backdrop: a slowly flowing, blurred copy of the cover over
// a gradient of its palette, a soft bloom of its brightest colours and a
// faint light wave (after the PS3's XrossMediaBar), toned so text stays
// legible on top. The fine sparkles round the wave are the app's, drawn
// over this at the window's resolution.
// Output is BGRA8 in display (gamma) space, as GPUI's atlas expects.

struct Params {
    // x: seconds, y: bass 0..1, z: kick 0..1, w: level 0..1
    audio: vec4<f32>,
    // xy: output size in pixels, z: light look (0/1), w: unused
    output: vec4<f32>,
    // x: weight of the new cover (cross-fade), y: has a cover (0/1),
    // z: the flow's clock (seconds times the swirl speed)
    cover: vec4<f32>,
    // The large cover's box under its drop shadow: left, top, right, bottom
    shadow_box: vec4<f32>,
    // x: the box's corner radius, y: pixels per point, z: the shadow's
    // alpha, w: on (0/1)
    shadow: vec4<f32>,
    // Strengths, 1 for the Default look: x: blur, y: the wave (0 off),
    // z: the wave's clock, w: bloom
    tune: vec4<f32>,
    // x: colour intensity, y: the wave's ribbons (1..3), z: the height of
    // its middle (0 top, 1 bottom)
    tune2: vec4<f32>,
    palette: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var cover_new: texture_2d<f32>;
@group(0) @binding(2) var cover_old: texture_2d<f32>;
@group(0) @binding(3) var cover_sampler: sampler;

// Dark look: the brightest the backdrop gets (relative luminance), so
// text_muted keeps 4.5:1 on top of it.
const DARK_CAP: f32 = 0.045;
// Dark look: colour pushed away from grey, so dim covers still tint it.
const DARK_SATURATION: f32 = 1.7;
// Light look: the luminance range the backdrop is pressed into. The floor
// keeps text_muted at 4.5:1 on top (it needs 0.63).
const LIGHT_FLOOR: f32 = 0.645;
const LIGHT_TOP: f32 = 0.8;
// Light look: how much of the colour the gamut allows is kept (a pastel).
const LIGHT_CHROMA: f32 = 0.9;
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

fn hash4(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(hash(p), hash(p + 19.19), hash(p + 47.3), hash(p + 83.7));
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}

fn fbm(p: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        value += amplitude * noise(q);
        q = q * 2.03 + vec2<f32>(1.7, 9.2);
        amplitude *= 0.5;
    }
    return value;
}

// The cover, cross-fading from the old one to the new one.
fn cover_at(uv: vec2<f32>) -> vec3<f32> {
    let new_color = textureSampleLevel(cover_new, cover_sampler, uv, 0.0).rgb;
    let old_color = textureSampleLevel(cover_old, cover_sampler, uv, 0.0).rgb;
    return mix(old_color, new_color, params.cover.x);
}

// A wide blur around `uv` (the cover is pre-blurred on upload): the centre
// and six taps on a ring.
fn wide_blur(uv: vec2<f32>, radius: f32) -> vec3<f32> {
    var sum = cover_at(uv);
    for (var i = 0; i < 6; i++) {
        let a = f32(i) * 1.0471976 + 0.5;
        sum += cover_at(uv + vec2<f32>(cos(a), sin(a)) * radius);
    }
    return sum / 7.0;
}

// Dark look: luminance compressed under DARK_CAP, hue kept, a floor so it
// never goes flat black.
fn tone_dark(color: vec3<f32>) -> vec3<f32> {
    let lin = pow(max(color, vec3<f32>(0.0)), vec3<f32>(2.2));
    let y = max(dot(lin, LUMA), 1e-4);
    let saturated = max(mix(vec3<f32>(y), lin, DARK_SATURATION * params.tune2.x), vec3<f32>(0.0));
    let target_y = 0.004 + (DARK_CAP - 0.004) * (1.0 - exp(-y * 9.0));
    let toned = saturated * (target_y / y);
    // Channels over 1 would break the cap's hue; they can't at these levels.
    return pow(toned, vec3<f32>(1.0 / 2.2));
}

// Light look: luminance lifted into LIGHT_FLOOR..LIGHT_TOP, hue kept. The
// colour's offset from grey is scaled as far as the gamut allows at the new
// luminance, then by LIGHT_CHROMA, so a dark blue cover gives a clear sky
// blue rather than a near-white wash.
fn tone_light(color: vec3<f32>) -> vec3<f32> {
    let lin = pow(max(color, vec3<f32>(0.0)), vec3<f32>(2.2));
    let y = max(dot(lin, LUMA), 1e-4);
    let target_y = mix(LIGHT_FLOOR, LIGHT_TOP, smoothstep(0.05, 0.6, y));
    // dot(offset, LUMA) is 0: scaling it leaves the luminance at target_y.
    let offset = lin - vec3<f32>(y);
    let up = max(max(offset.r, max(offset.g, offset.b)), 1e-4);
    let down = max(max(-offset.r, max(-offset.g, -offset.b)), 1e-4);
    let fit = min(target_y / y, min((1.0 - target_y) / up, target_y / down));
    // More than the gamut allows would clip and darken: at most all of it.
    let toned = vec3<f32>(target_y) + offset * fit * min(LIGHT_CHROMA * params.tune2.x, 1.0);
    return pow(clamp(toned, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
}

// The error function, as GPUI's shadows use it.
fn erf2(v: vec2<f32>) -> vec2<f32> {
    let s = sign(v);
    let a = abs(v);
    let r1 = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    let r2 = r1 * r1;
    return s - s / (r2 * r2);
}

// How much of a box blurred by a gaussian of `sigma` covers `p`.
fn blurred_box(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, sigma: f32) -> f32 {
    let k = 1.0 / (sigma * 1.4142135);
    let a = erf2((p - lo) * k);
    let b = erf2((p - hi) * k);
    return 0.25 * (a.x - b.x) * (a.y - b.y);
}

// The same for a rounded box, from its signed distance: exact along the
// edges, a little soft at the corners (fine for the tight layer).
fn blurred_round_box(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, r: f32, sigma: f32) -> f32 {
    let centre = (lo + hi) * 0.5;
    let q = abs(p - centre) - (hi - lo) * 0.5 + r;
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    return 0.5 - 0.5 * erf2(vec2<f32>(d / (sigma * 1.4142135), 0.0)).x;
}

// The cover's drop shadow (the theme's elevation "high": 2 pt down with a
// 6 pt blur at half the alpha, 16 pt down with a 40 pt blur): how much
// light is left at `p`.
fn cover_shadow(p: vec2<f32>) -> f32 {
    let lo = params.shadow_box.xy;
    let hi = params.shadow_box.zw;
    let s = params.shadow.y;
    let alpha = params.shadow.z;
    let tight = blurred_round_box(p, lo + vec2<f32>(0.0, 2.0 * s), hi + vec2<f32>(0.0, 2.0 * s),
        params.shadow.x, 6.0 * s);
    let wide = blurred_box(p, lo + vec2<f32>(0.0, 16.0 * s), hi + vec2<f32>(0.0, 16.0 * s), 40.0 * s);
    return (1.0 - tight * alpha * 0.5) * (1.0 - wide * alpha);
}

// The wave's ribbon `i` at `x` (0..1 across): its middle, in output
// heights from the top, slowly undulating with the wave's clock `t`. The
// app's sparkles follow the same curves (visuals::ambient::ribbon).
fn ribbon(i: i32, x: f32, t: f32) -> f32 {
    let k = f32(i);
    let a = sin(6.2832 * (x * (0.55 + 0.12 * k) + 0.23 * k) + t * (0.09 + 0.025 * k));
    let b = sin(6.2832 * (x * (1.15 - 0.2 * k) - 0.31 * k) - t * (0.05 + 0.02 * k));
    return params.tune2.z + 0.03 * (k - 1.0) + (0.1 - 0.02 * k) * a + 0.04 * b;
}

// The wave over `color`: soft translucent ribbons of the palette, each with
// a faint brighter crest; lighter than what is under them in either look.
fn with_wave(color: vec3<f32>, uv: vec2<f32>, light: bool) -> vec3<f32> {
    let strength = params.tune.y * select(1.0, 2.0, light);
    if strength <= 0.0 {
        return color;
    }
    var out = color;
    let ribbons = clamp(i32(params.tune2.y + 0.5), 1, 3);
    for (var i = 0; i < ribbons; i++) {
        let k = f32(i);
        let d = (uv.y - ribbon(i, uv.x, params.tune.z)) / (0.07 + 0.03 * k);
        let sheet = exp(-d * d) * 0.07 + exp(-d * d * 60.0) * 0.05;
        let tint = mix(params.palette[i].rgb, vec3<f32>(1.0), select(0.25, 0.65, light));
        out = mix(out, tint, clamp(sheet * strength * (1.0 - 0.25 * k), 0.0, 1.0));
    }
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let t = params.audio.x;
    let tf = params.cover.z;
    let bass = params.audio.y;
    let kick = params.audio.z;
    let light = params.output.z > 0.5;
    let aspect = params.output.x / max(params.output.y, 1.0);
    let p = (in.uv - 0.5) * vec2<f32>(aspect, 1.0);

    // Domain warp: two layers of noise push the coordinates around slowly.
    let warp = vec2<f32>(
        fbm(p * 1.3 + vec2<f32>(tf * 0.045, -tf * 0.035)),
        fbm(p * 1.3 + vec2<f32>(-tf * 0.03, tf * 0.05) + 5.2),
    );
    let flow = p + (warp - 0.5) * 0.85;

    // The cover, slowly turning and zoomed in, breathing with the bass.
    let angle = tf * 0.025;
    let rot = mat2x2<f32>(cos(angle), -sin(angle), sin(angle), cos(angle));
    let cover_uv = rot * flow * (0.6 - kick * 0.025) + 0.5;
    let near = cover_at(cover_uv);
    let wide = wide_blur(cover_uv, 0.16 * params.tune.x);

    // The palette as a flowing gradient (all there is without a cover).
    let n = fbm(flow * 1.8 + tf * 0.06);
    let g1 = mix(params.palette[0].rgb, params.palette[1].rgb, smoothstep(0.2, 0.8, n));
    let g2 = mix(params.palette[2].rgb, params.palette[3].rgb, smoothstep(0.3, 0.7, warp.x));
    let gradient = mix(g1, g2, smoothstep(0.2, 0.8, in.uv.y + (warp.y - 0.5) * 0.6));

    var color = mix(gradient, mix(near, wide, min(0.4 * params.tune.x, 1.0)), 0.7 * params.cover.y);

    // Bloom: the brightest colours of the wide blur glow, a little more on
    // the bass.
    let glow = wide * smoothstep(0.3, 0.85, dot(wide, LUMA));
    color += glow * (0.35 + 0.35 * bass) * params.tune.w;

    // The wave goes in before the tone mapping, which keeps it within the
    // range text stays legible on.
    color = with_wave(color, in.uv, light);

    if light {
        color = tone_light(color);
    } else {
        color = tone_dark(color);
        // Vignette: the edges sink a little.
        color *= 1.0 - 0.3 * dot(p, p);
    }


    // GPUI blends shadows in display space: the same here.
    if params.shadow.w > 0.5 {
        color *= cover_shadow(in.uv * params.output.xy);
    }

    // Dither so 8-bit gradients don't band.
    let grain = (hash(in.uv * params.output.xy + fract(t) * 61.0) - 0.5) / 255.0;
    return vec4<f32>(color + grain, 1.0);
}
