// The animated cover backdrop (M8 spike): a slowly flowing, heavily blurred
// copy of the cover, tinted by the cover's palette, with a bass pulse.
// Output is BGRA8 in display (gamma) space, as GPUI's atlas expects.

struct Params {
    // x: seconds, y: bass 0..1, z: overall level 0..1, w: has cover (0/1)
    time: vec4<f32>,
    // xy: output size in pixels
    size: vec4<f32>,
    palette: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var cover: texture_2d<f32>;
@group(0) @binding(2) var cover_sampler: sampler;

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

// A wide blur of the (already tiny) cover: 12 taps on two rings.
fn blurred_cover(uv: vec2<f32>) -> vec3<f32> {
    var sum = textureSample(cover, cover_sampler, uv).rgb;
    for (var i = 0; i < 6; i++) {
        let a = f32(i) * 1.0471976;
        let d = vec2<f32>(cos(a), sin(a));
        sum += textureSample(cover, cover_sampler, uv + d * 0.06).rgb;
        sum += textureSample(cover, cover_sampler, uv + d.yx * 0.13).rgb;
    }
    return sum / 13.0;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let t = params.time.x;
    let bass = params.time.y;
    let aspect = params.size.x / max(params.size.y, 1.0);
    var p = (in.uv - 0.5) * vec2<f32>(aspect, 1.0);

    // Domain warp: two layers of noise push the coordinates around slowly.
    let warp = vec2<f32>(
        fbm(p * 1.4 + vec2<f32>(t * 0.05, -t * 0.04)),
        fbm(p * 1.4 + vec2<f32>(-t * 0.03, t * 0.06) + 5.2),
    );
    let flow = p + (warp - 0.5) * 0.9;

    // The cover, slowly turning and zoomed in, blurred.
    let angle = t * 0.03;
    let rot = mat2x2<f32>(cos(angle), -sin(angle), sin(angle), cos(angle));
    let cover_uv = rot * flow * (0.55 - bass * 0.04) + 0.5;
    let from_cover = blurred_cover(cover_uv);

    // The palette as a flowing gradient (the fallback without a cover).
    let n = fbm(flow * 2.0 + t * 0.07);
    let g1 = mix(params.palette[0].rgb, params.palette[1].rgb, smoothstep(0.2, 0.8, n));
    let g2 = mix(params.palette[2].rgb, params.palette[3].rgb, smoothstep(0.3, 0.7, warp.x));
    let gradient = mix(g1, g2, smoothstep(0.25, 0.75, in.uv.y + (warp.y - 0.5) * 0.6));

    var color = mix(gradient, from_cover, 0.65 * params.time.w);

    // Darken for text on top, lift with the bass, vignette.
    let vignette = 1.0 - 0.55 * dot(p, p);
    color = color * (0.7 + 0.25 * bass) * vignette;

    // Dither so 8-bit gradients don't band.
    let grain = (hash(in.uv * params.size.xy + fract(t) * 61.0) - 0.5) / 255.0;
    return vec4<f32>(color + grain, 1.0);
}
