// A track change on a cover: the old cover burns away into the new one
// along a noise front, with a thin glowing edge in the new cover's accent.
// Output is BGRA8 in display (gamma) space.

struct Params {
    // x: progress 0..1, y: light look (0/1), zw: output size in pixels
    state: vec4<f32>,
    // The edge's colour (display space).
    accent: vec4<f32>,
    // How much of each cover's width and height shows (the view crops a
    // wide cover to the square): xy the old cover, zw the new one.
    crop: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var cover_old: texture_2d<f32>;
@group(0) @binding(2) var cover_new: texture_2d<f32>;
@group(0) @binding(3) var cover_sampler: sampler;

// The front's softness and the glowing edge's width, in noise units.
const SOFT: f32 = 0.025;
const EDGE: f32 = 0.06;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

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

// Three octaves, in cover units: it looks the same on a 56 point thumb and
// a 400 point cover.
fn fbm(uv: vec2<f32>) -> f32 {
    var p = uv * 4.0;
    var sum = 0.0;
    var amp = 0.55;
    for (var i = 0; i < 3; i++) {
        sum += noise(p) * amp;
        p = p * 2.03 + vec2<f32>(5.2, 1.3);
        amp *= 0.5;
    }
    return sum / 0.9625;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let centred = in.uv - 0.5;
    let before = textureSampleLevel(cover_old, cover_sampler, 0.5 + centred * params.crop.xy, 0.0).rgb;
    let after = textureSampleLevel(cover_new, cover_sampler, 0.5 + centred * params.crop.zw, 0.0).rgb;
    // A slight sweep from the top left, so the burn has a direction.
    let n = fbm(in.uv) * 0.8 + (in.uv.x + in.uv.y) * 0.1;
    let t = params.state.x;
    // The front runs from below the smallest value to past the largest.
    let front = mix(-EDGE - SOFT, 1.0 + EDGE + SOFT, t);
    let shown = 1.0 - smoothstep(front - SOFT, front + SOFT, n);
    var color = mix(before, after, shown);
    let edge = 1.0 - smoothstep(0.0, EDGE, abs(n - front));
    let glow = select(1.0, 0.75, params.state.y > 0.5) * edge * (1.0 - shown * 0.6);
    color = mix(color, params.accent.rgb, clamp(glow, 0.0, 1.0));
    return vec4<f32>(color, 1.0);
}
