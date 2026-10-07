// The audio visualiser: bars, mirrored bars, a ring round the cover, a line
// spectrum or a particle field, from the bars the app worked out of the
// spectrum; and the backdrop's ambient layer (style 5): a soft flowing light
// wave and fine sparkles drifting along it. Drawn with an alpha channel (straight, not premultiplied, as
// GPUI's sprites take it) over the backdrop. Positions are in output
// pixels. Colour is BGRA8 in display (gamma) space.

struct Params {
    // xy: output size in pixels, z: style (0 bars, 1 mirrored, 2 ring,
    // 3 line, 4 particles, 5 ambient), w: bar count
    output: vec4<f32>,
    // x: seconds, y: bass, z: kick, w: level (0..1)
    audio: vec4<f32>,
    // x: opacity, y: glow 0..2, z: peak caps (0/1), w: light look (0/1)
    look: vec4<f32>,
    // The cover: left, top, right, bottom
    cover: vec4<f32>,
    // x: the cover's corner radius, y: the ring's longest bar, z: the
    // particles' clock, w: treble 0..1
    shape: vec4<f32>,
    // x: pixels per point, y: has a cover (0/1)
    extra: vec4<f32>,
    // The ambient layer (style 5). Sparkles: x: amount 0..2, y: smallest
    // and z: largest radius in device pixels, w: softness 0..1
    ambient: vec4<f32>,
    // x: brightness 0..2, y: the drift's clock, z: depth spread 0..1,
    // w: music reaction 0..1
    ambient2: vec4<f32>,
    // x: twinkle 0..1, y: the twinkle's clock, z: drift direction
    // (radians, 0 rightwards, counter-clockwise), w: colour tint 0..1
    ambient3: vec4<f32>,
    // The wave: x: strength 0..2 (0 off), y: its clock, z: ribbons 1..3,
    // w: height of its middle (0 top, 1 bottom)
    wave: vec4<f32>,
    // The gradient along the spectrum, linear RGB, low to high.
    stops: array<vec4<f32>, 4>,
    // Four bars to a vec4: heights, then peak caps, 0..1.
    bars: array<vec4<f32>, 32>,
    peaks: array<vec4<f32>, 32>,
};

@group(0) @binding(0) var<uniform> params: Params;

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

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn hash4(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(hash(p), hash(p + 19.19), hash(p + 47.3), hash(p + 83.7));
}

fn count() -> i32 {
    return max(i32(params.output.w), 1);
}

fn bar(i: i32) -> f32 {
    let j = u32(clamp(i, 0, count() - 1));
    return params.bars[j >> 2u][j & 3u];
}

fn peak(i: i32) -> f32 {
    let j = u32(clamp(i, 0, count() - 1));
    return params.peaks[j >> 2u][j & 3u];
}

// The gradient at `u` (0 lows, 1 highs), linear RGB.
fn gradient(u: f32) -> vec3<f32> {
    let x = clamp(u, 0.0, 1.0) * 3.0;
    let i = u32(min(floor(x), 2.0));
    return mix(params.stops[i].rgb, params.stops[i + 1u].rgb, smoothstep(0.0, 1.0, x - f32(i)));
}

// Signed distance to a box from `lo` to `hi` with corners rounded by `r`.
fn round_box(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, r: f32) -> f32 {
    let centre = (lo + hi) * 0.5;
    let half = max((hi - lo) * 0.5, vec2<f32>(0.0));
    let rr = min(r, min(half.x, half.y));
    let q = abs(p - centre) - half + rr;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rr;
}

// A colour and how much of it covers the pixel (straight alpha).
struct Ink {
    color: vec3<f32>,
    alpha: f32,
};

// `top` over `under`.
fn over(under: Ink, color: vec3<f32>, alpha: f32) -> Ink {
    let a = clamp(alpha, 0.0, 1.0);
    let out_a = a + under.alpha * (1.0 - a);
    if out_a < 1e-4 {
        return Ink(color, 0.0);
    }
    let c = (color * a + under.color * under.alpha * (1.0 - a)) / out_a;
    return Ink(c, out_a);
}

// The tip of a bar is a little lighter in the dark look.
fn lit(color: vec3<f32>, up: f32) -> vec3<f32> {
    if params.look.w > 0.5 {
        return color;
    }
    return mix(color, vec3<f32>(1.0), 0.22 * clamp(up, 0.0, 1.0));
}

// A peak cap's colour: lighter than its bar.
fn cap_color(color: vec3<f32>) -> vec3<f32> {
    if params.look.w > 0.5 {
        return color * 0.8;
    }
    return mix(color, vec3<f32>(1.0), 0.55);
}

// A glow's reach, in output pixels.
fn glow_radius() -> f32 {
    return (3.0 + 5.0 * params.look.y) * params.extra.x;
}

// One bar of a row: its body (rounded, from `base` up or down by its
// height), its glow and its peak cap. `dir` is -1 for up, 1 for down; `fade`
// dims a reflection.
fn row_bar(ink: Ink, p: vec2<f32>, i: i32, centre: f32, half_w: f32, base: f32, room: f32,
    dir: f32, fade: f32) -> Ink {
    let s = params.extra.x;
    let v = bar(i);
    let u = (f32(i) + 0.5) / f32(count());
    let color = gradient(u);
    let len = max(v * room, 2.0 * s);
    let tip = base + dir * len;
    let lo = vec2<f32>(centre - half_w, min(base, tip));
    let hi = vec2<f32>(centre + half_w, max(base, tip));
    let d = round_box(p, lo, hi, min(half_w, 3.0 * s));
    var out = ink;
    let g = params.look.y;
    if g > 0.0 {
        let r = glow_radius();
        let halo = exp(-max(d, 0.0) / r) * step(0.0, d) * (0.12 + 0.4 * v) * min(g, 1.5) * 0.5;
        out = over(out, color, halo * fade);
    }
    let up = abs(p.y - base) / max(room, 1.0);
    // A bar at rest is a faint stub.
    let rest = mix(0.35, 1.0, smoothstep(0.0, 0.06, v));
    out = over(out, lit(color, up), clamp(0.5 - d, 0.0, 1.0) * fade * rest);
    if params.look.z > 0.5 {
        let pk = peak(i) * room;
        let gap = 2.0 * s;
        let cap = 2.0 * s;
        let y0 = base + dir * (max(pk, len) + gap);
        let y1 = y0 + dir * cap;
        let cd = round_box(p, vec2<f32>(centre - half_w, min(y0, y1)),
            vec2<f32>(centre + half_w, max(y0, y1)), cap * 0.5);
        let shown = smoothstep(0.01, 0.04, peak(i));
        out = over(out, cap_color(color), clamp(0.5 - cd, 0.0, 1.0) * fade * shown);
    }
    return out;
}

// Bars rising from the bottom, lows at the left.
fn bars(p: vec2<f32>) -> Ink {
    let size = params.output.xy;
    let n = count();
    let pitch = size.x / f32(n);
    let i = i32(floor(p.x / pitch));
    let half_w = max(pitch * 0.34, 0.5 * params.extra.x);
    var ink = Ink(gradient(0.0), 0.0);
    // The neighbours' glow reaches into this column.
    for (var k = -1; k <= 1; k++) {
        let j = i + k;
        if j < 0 || j >= n {
            continue;
        }
        let centre = (f32(j) + 0.5) * pitch;
        ink = row_bar(ink, p, j, centre, half_w, size.y - 1.0, size.y * 0.94 - 8.0 * params.extra.x,
            -1.0, 1.0);
    }
    return ink;
}

// Bars mirrored round the centre (lows in the middle), growing up from the
// centre line, with a dimmer reflection below.
fn mirrored(p: vec2<f32>) -> Ink {
    let size = params.output.xy;
    let n = count();
    let half_x = size.x * 0.5;
    let pitch = half_x / f32(n);
    let x = abs(p.x - half_x);
    let i = i32(floor(x / pitch));
    let half_w = max(pitch * 0.34, 0.5 * params.extra.x);
    let base = size.y * 0.62;
    let room_up = base - 8.0 * params.extra.x;
    let room_down = (size.y - base) - 4.0 * params.extra.x;
    var ink = Ink(gradient(0.0), 0.0);
    let q = vec2<f32>(x, p.y);
    for (var k = -1; k <= 1; k++) {
        let j = i + k;
        if j < 0 || j >= n {
            continue;
        }
        let centre = (f32(j) + 0.5) * pitch;
        if p.y <= base {
            ink = row_bar(ink, q, j, centre, half_w, base - 1.0 * params.extra.x, room_up, -1.0, 1.0);
        } else {
            let fade = 0.35 * (1.0 - smoothstep(0.0, room_down, p.y - base));
            ink = row_bar(ink, q, j, centre, half_w, base + 1.0 * params.extra.x, room_down, 1.0, fade);
        }
    }
    return ink;
}

// The smooth curve through the bars at `x` (Catmull-Rom).
fn curve_value(x: f32, values_peak: bool) -> f32 {
    let n = count();
    let f = clamp(x / params.output.x, 0.0, 1.0) * f32(n - 1);
    let i = i32(floor(f));
    let t = f - f32(i);
    var p0 = bar(i - 1);
    var p1 = bar(i);
    var p2 = bar(i + 1);
    var p3 = bar(i + 2);
    if values_peak {
        p0 = peak(i - 1);
        p1 = peak(i);
        p2 = peak(i + 1);
        p3 = peak(i + 2);
    }
    let t2 = t * t;
    let t3 = t2 * t;
    let v = 0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
    return clamp(v, 0.0, 1.0);
}

// A line spectrum: a smooth curve with the area under it filled, fading
// towards the bottom.
fn line(p: vec2<f32>) -> Ink {
    let size = params.output.xy;
    let s = params.extra.x;
    let room = size.y * 0.9 - 4.0 * s;
    let v = curve_value(p.x, false);
    let y = size.y - 2.0 * s - v * room;
    // The curve's slope, for an even stroke width.
    let dy = (curve_value(p.x + 1.0, false) - curve_value(p.x - 1.0, false)) * 0.5 * room;
    let d = abs(p.y - y) / sqrt(1.0 + dy * dy);
    let u = p.x / size.x;
    let color = gradient(u);
    var ink = Ink(color, 0.0);
    if p.y > y {
        let depth = (p.y - y) / max(size.y - y, 1.0);
        let fill = mix(0.42, 0.04, sqrt(depth)) * (0.6 + 0.4 * v);
        ink = over(ink, color, fill);
    }
    let g = params.look.y;
    if g > 0.0 {
        let halo = exp(-d / (glow_radius() * 1.4)) * (0.25 + 0.5 * v) * min(g, 1.5) * 0.7;
        ink = over(ink, color, halo);
    }
    let width = 1.25 * s;
    ink = over(ink, lit(color, v), clamp(width + 0.5 - d, 0.0, 1.0));
    if params.look.z > 0.5 {
        let pv = curve_value(p.x, true);
        let py = size.y - 2.0 * s - pv * room - 3.0 * s;
        let pd = abs(p.y - py);
        let dash = step(0.45, fract(p.x / (6.0 * s)));
        ink = over(ink, cap_color(color), clamp(0.5 * s + 0.5 - pd, 0.0, 1.0) * 0.7 * dash
            * smoothstep(0.02, 0.06, pv));
    }
    return ink;
}

// Where `p` is along the cover's outline (x: 0..1, clockwise from the top
// centre), how far outside it (y), and how much wider a step along the
// outline is out there than on it (z: more than 1 round the corners), for a
// box with corners rounded by `r`.
fn outline(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, r: f32) -> vec3<f32> {
    let centre = (lo + hi) * 0.5;
    let half = (hi - lo) * 0.5;
    let rr = min(r, min(half.x, half.y));
    let a = half - rr;
    let q = p - centre;
    let c = clamp(q, -a, a);
    let v = q - c;
    let d = length(v) - rr;
    let arc = rr * PI * 0.5;
    let total = 4.0 * (a.x + a.y) + 4.0 * arc;
    var pos = 0.0;
    let corner = (rr + max(d, 0.0)) / max(rr, 1e-3);
    var stretch = 1.0;
    if q.y < -a.y && abs(q.x) <= a.x {
        pos = q.x;
    } else if q.x > a.x && q.y < -a.y {
        pos = a.x + rr * atan2(v.x, -v.y);
    } else if q.x > a.x && abs(q.y) <= a.y {
        pos = a.x + arc + (q.y + a.y);
    } else if q.x > a.x && q.y > a.y {
        pos = a.x + arc + 2.0 * a.y + rr * atan2(v.y, v.x);
    } else if q.y > a.y && abs(q.x) <= a.x {
        pos = a.x + 2.0 * arc + 2.0 * a.y + (a.x - q.x);
    } else if q.x < -a.x && q.y > a.y {
        pos = 3.0 * a.x + 2.0 * arc + 2.0 * a.y + rr * atan2(-v.x, v.y);
    } else if q.x < -a.x && abs(q.y) <= a.y {
        pos = 3.0 * a.x + 3.0 * arc + 2.0 * a.y + (a.y - q.y);
    } else if q.x < -a.x && q.y < -a.y {
        pos = 3.0 * a.x + 3.0 * arc + 4.0 * a.y + rr * atan2(-v.y, -v.x);
    } else {
        // Inside the cover: hidden under it.
        return vec3<f32>(0.0, -1.0, 1.0);
    }
    if abs(q.x) > a.x && abs(q.y) > a.y {
        stretch = corner;
    }
    return vec3<f32>(fract(pos / total + 1.0), d, stretch);
}

// Bars standing out of the cover's outline all round it, mirrored left and
// right (lows at the top), over a soft aura that breathes with the bass.
fn ring(p: vec2<f32>) -> Ink {
    let s = params.extra.x;
    // The bars stand on an outline a little outside the cover, rounder
    // than it, so the corners fan out gently.
    let gap = 3.0 * s + 0.03 * (params.cover.z - params.cover.x);
    let lo = params.cover.xy - gap;
    let hi = params.cover.zw + gap;
    let r = params.shape.x + gap;
    let reach = params.shape.y;
    let o = outline(p, lo, hi, r);
    var ink = Ink(gradient(0.0), 0.0);
    if o.y < 0.0 {
        return ink;
    }
    let n = count();
    // Half the outline holds the bars, the other half mirrors them.
    let side = max(n / 2, 1);
    let u = min(o.x, 1.0 - o.x) * 2.0;
    let perimeter = 2.0 * ((hi.x - lo.x) + (hi.y - lo.y)) - (8.0 - 2.0 * PI) * r;
    let pitch = perimeter * 0.5 / f32(side);
    let f = u * f32(side);
    let i = i32(min(floor(f), f32(side - 1)));
    let across = (f - (f32(i) + 0.5)) * pitch * o.z;
    let along = o.y;
    let color = gradient(u);
    // The aura: a soft glow off the cover's edge, more on the bass.
    let g = params.look.y;
    let aura = exp(-o.y / max(reach * 0.45, 1.0)) * (0.1 + 0.35 * params.audio.y) * min(g, 1.5);
    ink = over(ink, color, aura * 0.8);
    let k = i * n / side;
    let v = bar(k);
    let half_w = max(pitch * 0.32, 0.6 * s);
    let len = max(v * reach, 1.5 * s);
    let d = round_box(vec2<f32>(across, along), vec2<f32>(-half_w, 0.0), vec2<f32>(half_w, len),
        min(half_w, 3.0 * s));
    if g > 0.0 {
        let halo = exp(-max(d, 0.0) / glow_radius()) * step(0.0, d) * (0.15 + 0.5 * v) * min(g, 1.5) * 0.6;
        ink = over(ink, color, halo);
    }
    let rest = mix(0.3, 1.0, smoothstep(0.0, 0.06, v));
    ink = over(ink, lit(color, along / max(reach, 1.0)), clamp(0.5 - d, 0.0, 1.0) * rest);
    if params.look.z > 0.5 {
        let pk = max(peak(k) * reach, len) + 2.0 * s;
        let cd = round_box(vec2<f32>(across, along), vec2<f32>(-half_w, pk),
            vec2<f32>(half_w, pk + 2.0 * s), s);
        ink = over(ink, cap_color(color), clamp(0.5 - cd, 0.0, 1.0) * smoothstep(0.01, 0.04, peak(k)));
    }
    return ink;
}

// A field of glowing particles drifting up, each tied to one bar: as large
// and bright as its bar, flaring on the kick. Three layers at different
// depths.
fn particles(p: vec2<f32>) -> Ink {
    let size = params.output.xy;
    let uv = p / size.y;
    let travel = params.shape.z;
    let kick = params.audio.z;
    let n = count();
    var ink = Ink(gradient(0.5), 0.0);
    for (var layer = 0; layer < 3; layer++) {
        let depth = f32(layer);
        let cells = 7.0 + depth * 5.0;
        let speed = 0.05 + depth * 0.035;
        let q = vec2<f32>(uv.x, uv.y + travel * speed) * cells;
        let cell = floor(q);
        let h = hash4(cell + depth * 17.0);
        if h.w < 0.3 {
            continue;
        }
        let i = i32(h.z * f32(n));
        let v = bar(i);
        let t = params.audio.x;
        let sway = vec2<f32>(sin(t * 0.7 + h.x * 6.28), cos(t * 0.5 + h.y * 6.28)) * 0.05;
        let centre = 0.3 + 0.4 * h.xy + sway;
        let d = length(fract(q) - centre);
        let core = (0.025 + 0.07 * v) * (1.0 + 0.6 * kick);
        let color = gradient((f32(i) + 0.5) / f32(n));
        let near = 1.0 - 0.3 * depth;
        let g = params.look.y;
        if g > 0.0 {
            let sigma = core * 1.6 + 0.02;
            let halo = exp(-(d * d) / (sigma * sigma)) * (0.15 + 0.6 * v) * min(g, 1.5) * 0.6 * near;
            ink = over(ink, color, halo);
        }
        let px = cells / size.y;
        let body = clamp((core - d) / px + 0.5, 0.0, 1.0) * (0.35 + 0.65 * v) * near;
        ink = over(ink, lit(color, v), body);
    }
    return ink;
}

// The wave's ribbon `i` at `x` (0..1 across): its middle, in output
// heights from the top, slowly undulating with the wave's clock `t`.
fn ribbon(i: i32, x: f32, t: f32) -> f32 {
    let k = f32(i);
    let a = sin(6.2832 * (x * (0.55 + 0.12 * k) + 0.23 * k) + t * (0.09 + 0.025 * k));
    let b = sin(6.2832 * (x * (1.15 - 0.2 * k) - 0.31 * k) - t * (0.05 + 0.02 * k));
    return params.wave.w + 0.03 * (k - 1.0) + (0.1 - 0.02 * k) * a + 0.04 * b;
}

fn ribbons() -> i32 {
    return clamp(i32(params.wave.z + 0.5), 1, 3);
}

// How close a point at height `y` (output heights) is to the wave, 0..1.
fn near_wave(x: f32, y: f32) -> f32 {
    var d = 1.0;
    for (var i = 0; i < ribbons(); i++) {
        d = min(d, abs(y - ribbon(i, x, params.wave.y)));
    }
    return exp(-(d * d) / 0.03);
}

// The ambient layer (the backdrop's, after the PS3's XrossMediaBar): soft,
// translucent ribbons of the cover's colours flowing slowly across, and
// fine sparkles, mostly a pixel or two, drifting with them. Each sparkle is
// one per grid cell in three layers of depth (far ones smaller, fainter
// and slower, as far as the depth spread says), from a hash of its cell
// that stays put while the grid drifts, so nothing shimmers; it twinkles
// over seconds and gathers loosely round the wave. The music lifts the
// brightness a little, no more.
fn ambient(p: vec2<f32>) -> Ink {
    let size = params.output.xy;
    let s = params.extra.x;
    let uv = p / size;
    let light = params.look.w > 0.5;
    // White shows less on the light look's pale backdrop: twice as much.
    let boost = select(1.0, 2.0, light);
    var ink = Ink(vec3<f32>(1.0), 0.0);
    let wave = params.wave.x * boost;
    if wave > 0.0 {
        for (var i = 0; i < ribbons(); i++) {
            let k = f32(i);
            let d = (uv.y - ribbon(i, uv.x, params.wave.y)) / (0.07 + 0.03 * k);
            // A soft sheet with a faint brighter crest.
            let sheet = exp(-d * d) * 0.07 + exp(-d * d * 60.0) * 0.05;
            let color = mix(params.stops[i].rgb, vec3<f32>(1.0), select(0.25, 0.65, light));
            ink = over(ink, color, sheet * wave * (1.0 - 0.25 * k));
        }
    }
    let amount = params.ambient.x;
    if amount <= 0.0 {
        return ink;
    }
    let q = p / s;
    let spread = params.ambient2.z;
    let angle = params.ambient3.z;
    let heading = vec2<f32>(cos(angle), -sin(angle));
    let lift = 1.0 + 0.5 * params.ambient2.w * params.audio.w;
    for (var layer = 0; layer < 3; layer++) {
        // 0 far, 1 near; with no spread every layer sits in the middle.
        let near = mix(0.5, f32(layer) * 0.5, spread);
        // Cell size in points, drift in points a second.
        let cell = 16.0 + 18.0 * near;
        let drift = 3.0 + 7.0 * near;
        let g = (q - heading * params.ambient2.y * drift) / cell;
        let c = floor(g);
        // 97 cells round: the hashes repeat, so a long drift stays exact.
        let cw = c - 97.0 * floor(c / 97.0);
        let h = hash4(cw + f32(layer) * 41.0);
        if h.w < 1.0 - 0.32 * amount {
            continue;
        }
        let tw = params.ambient3.y;
        let sway = vec2<f32>(sin(tw * 0.21 + h.z * 6.28), cos(tw * 0.17 + h.x * 6.28)) * 0.07;
        let centre = 0.25 + 0.5 * h.xy + sway;
        // Its distance in device pixels.
        let d = length(fract(g) - centre) * cell * s;
        // One in fifteen a little larger.
        let big = select(1.0, 1.5, h.z > 0.93);
        let r = max(mix(params.ambient.y, params.ambient.z, near) * big, 0.5);
        let soft = exp(-(d * d) / (r * r));
        let hard = clamp(r - d + 0.5, 0.0, 1.0);
        let core = mix(hard, soft, params.ambient.w);
        // A slow fade in and out, seconds long, never a blink.
        let phase = tw * (0.5 + 0.6 * h.y) + h.w * 40.0;
        let twinkle = mix(1.0, 0.2 + 0.8 * smoothstep(-1.0, 1.0, sin(phase)), params.ambient3.x);
        let at = ((c + centre) * cell + heading * params.ambient2.y * drift) * s / size;
        let gather = mix(0.3, 1.0, near_wave(at.x, at.y));
        let bright = (0.28 + 0.4 * near) * twinkle * gather * lift * params.ambient2.x * boost;
        let tint = params.ambient3.w * select(0.3, 0.15, light);
        let color = mix(vec3<f32>(1.0), gradient(h.x), tint);
        // Never a solid dot, however bright the settings.
        ink = over(ink, color, min(core * bright, 0.8));
    }
    return ink;
}

// Display space from linear, per channel.
fn to_display(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, x <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let p = in.uv * params.output.xy;
    let style = i32(params.output.z + 0.5);
    var ink: Ink;
    if style == 0 {
        ink = bars(p);
    } else if style == 1 {
        ink = mirrored(p);
    } else if style == 2 {
        ink = ring(p);
    } else if style == 3 {
        ink = line(p);
    } else if style == 4 {
        ink = particles(p);
    } else {
        ink = ambient(p);
    }
    // Glows fade out before the frame's edge, so its box never shows:
    // all round for the ring and the particles, at the top for the bands
    // (their bars stand on the bottom edge).
    let size = params.output.xy;
    let margin = 14.0 * params.extra.x;
    var edge = p.y;
    if style == 2 || style == 4 {
        edge = min(min(p.x, size.x - p.x), min(p.y, size.y - p.y));
    } else if style == 5 {
        // The ambient layer fills its panel.
        edge = margin;
    }
    let alpha = clamp(ink.alpha * params.look.x, 0.0, 1.0) * smoothstep(0.0, margin, edge);
    return vec4<f32>(to_display(ink.color), alpha);
}
