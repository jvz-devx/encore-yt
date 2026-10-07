// Concept 1, a world per song, built from the song itself: the flight
// follows playback through a land laid out by the music (songmap.js bakes
// the decoded track into a texture). Along the path is the song's time, so
// what is coming shows on the horizon; across it the notes sounding then
// stand as ranges in circle-of-fifths order around the key; loudness
// raises the land and quiet passages sink below the water; bright timbre
// roughens it and snows the peaks; the song's character picks the biome
// and its mode the time of day (major: day and golden hour; minor: dusk).
// The cover gives the colours; the video id only moves the fine noise.
//
// Colour: materials keep natural lightness (grass, rock, sand, snow) and
// take their hue from the cover in OKLab: the sky from its top quadrants
// (palette 0, 1), the ground from its bottom ones (2, 3).

const M2: mat2x2<f32> = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
const FAR: f32 = 90.0;

@group(0) @binding(1) var song_map: texture_2d<f32>;
@group(0) @binding(2) var song_sampler: sampler;
// The land baked once per song by fs_bake (see the end of this file).
@group(0) @binding(3) var height_map: texture_2d<f32>;

// The baked land spans BAKE_HALF units either side of the path, from
// BAKE_BEFORE units before the song's start to BAKE_AFTER past its end
// (scenes.js uses the same numbers to size it).
const BAKE_HALF: f32 = 32.0;
const BAKE_BEFORE: f32 = 16.0;
const BAKE_AFTER: f32 = 40.0;
// Octaves in the bake; finer ones are added in the shading only.
const BAKE_OCTAVES: i32 = 6;

// A cheaper hash than common.wgsl's PCG for the terrain's many samples
// (Dave Hoskins' hash12); fine within the few thousand units a flight
// covers.
fn world_hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Value noise with derivatives: x the value 0..1, yz its gradient.
fn world_noised(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let a = world_hash(i);
    let b = world_hash(i + vec2<f32>(1.0, 0.0));
    let c = world_hash(i + vec2<f32>(0.0, 1.0));
    let d = world_hash(i + vec2<f32>(1.0, 1.0));
    let k1 = b - a;
    let k2 = c - a;
    let k4 = a - b - c + d;
    return vec3<f32>(a + k1 * u.x + k2 * u.y + k4 * u.x * u.y,
        du * (vec2<f32>(k1, k2) + k4 * u.yx));
}

fn world_fbm(p: vec2<f32>, octaves: i32) -> f32 {
    var q = p;
    var a = 0.0;
    var b = 0.5;
    for (var i = 0; i < octaves; i++) {
        a += b * world_noised(q).x;
        q = M2 * q * 2.03;
        b *= 0.5;
    }
    return a;
}

// How fast the flight covers the song: units per second of playback.
const SPEED: f32 = 1.5;
// Half the width of the valley the song map spans, in units.
const HALF_WIDTH: f32 = 20.0;
const ROWS_PER_SECOND: f32 = 4.0;

// The land the song gives. Biomes (from the whole song's character, see
// songmap.js): 0 rolling hills, 1 alpine, 2 mesa desert, 3 islands.
struct Land {
    biome: i32,
    height: f32,
    // Ridged octaves: 0 soft, 1 sharp.
    crag: f32,
    // How strongly slopes damp the finer octaves (eroded look).
    erosion: f32,
    terrace: f32,
    dunes: f32,
    water: f32,
    offset: vec2<f32>,
    forward: vec2<f32>,
    side: vec2<f32>,
};

fn land() -> Land {
    let s = params.seed;
    var l: Land;
    l.biome = clamp(i32(params.clock.w + 0.5), 0, 3);
    // The video id only moves the fine noise and the heading.
    // Fixed, so the bake depends only on the song.
    l.offset = vec2<f32>(137.0, 291.0);
    l.forward = vec2<f32>(0.0, 1.0);
    l.side = vec2<f32>(-1.0, 0.0);
    l.terrace = 0.0;
    l.dunes = 0.0;
    switch l.biome {
        case 0: {
            l.height = 3.2;
            l.crag = 0.0;
            l.erosion = 0.6;
            l.water = -0.35;
        }
        case 1: {
            l.height = 6.5;
            l.crag = 0.45;
            l.erosion = 1.0;
            l.water = -0.9;
        }
        case 2: {
            l.height = 4.0;
            l.crag = 0.1;
            l.erosion = 0.8;
            l.terrace = 0.75;
            l.dunes = 1.0;
            l.water = -100.0;
        }
        default: {
            l.height = 4.2;
            l.crag = 0.15;
            l.erosion = 0.8;
            l.water = 0.15;
        }
    }
    return l;
}

// The song map at a ground point: r the ranges' height, g brightness, b
// energy, a onsets. Along the flight is the song's time, across is the
// harmony's lanes; before the song starts and after it ends the land
// falls quiet, and outside the valley it settles to a mean.
fn song_at(p: vec2<f32>, l: Land) -> vec4<f32> {
    if params.clock.z <= 0.0 {
        return vec4<f32>(0.3, 0.45, 0.45, 0.2);
    }
    let rows = f32(textureDimensions(song_map).y);
    // The lanes meander a little, so ranges wind like real ones.
    let warp = world_noised(p * 0.045 + l.offset).x - 0.5;
    let row = dot(p, l.forward) / SPEED * ROWS_PER_SECOND;
    let across = (dot(p, l.side) + warp * 7.0) / HALF_WIDTH;
    let uv = vec2<f32>(clamp(across * 0.5 + 0.5, 0.0, 1.0), clamp((row + 0.5) / rows, 0.0, 1.0));
    let m = textureSampleLevel(song_map, song_sampler, uv, 0.0);
    let valley = smoothstep(1.25, 0.95, abs(across));
    let settled = vec4<f32>(0.22, m.g, m.b * 0.5, 0.1);
    let land_here = mix(settled, m, valley * smoothstep(-24.0, 0.0, row));
    // After the last row the land runs down into the sea: the song ends
    // at the shore.
    let quiet = vec4<f32>(0.05, m.g, 0.0, 0.0);
    return mix(quiet, land_here, smoothstep(rows + 24.0, rows, row));
}

// Height of the ground at `p` with `octaves` of detail: the song's ranges
// (the map) carry the shape, derivative-damped fbm (Inigo Quilez's
// terrain) makes them look like land, ridged for crags, terraced for
// mesas, with dunes in the desert's low ground. Bright, busy music
// roughens it.
fn terrain_procedural(p: vec2<f32>, octaves: i32, l: Land) -> f32 {
    let m = song_at(p, l);
    var q = p * 0.2 + l.offset;
    var a = 0.0;
    var b = 1.0;
    var d = vec2<f32>(0.0);
    for (var i = 0; i < octaves; i++) {
        let n = world_noised(q);
        d += n.yz;
        let ridge = 1.0 - abs(n.x * 2.0 - 1.0);
        let v = mix(n.x, ridge * ridge, l.crag);
        a += b * v / (1.0 + dot(d, d) * l.erosion);
        b *= 0.5;
        q = M2 * q * 2.0;
    }
    let rough = 0.2 + 0.25 * m.g + 0.15 * m.a + 0.25 * m.r;
    var h = m.r + (a - 0.5) * rough;
    if l.terrace > 0.0 {
        let k = h * 6.0;
        let stepped = (floor(k) + smoothstep(0.55, 0.95, fract(k))) / 6.0;
        h = mix(h, stepped, l.terrace);
    }
    if l.dunes > 0.0 && octaves > 3 {
        let w = p * 0.9 + vec2<f32>(world_noised(p * 0.15).x * 4.0, 0.0);
        let crest = 1.0 - abs(sin(w.x * 1.2 + w.y * 0.35));
        h += 0.03 * crest * crest * smoothstep(0.3, 0.05, h);
    }
    return (h - 0.2) * l.height;
}

// The length of land the bake covers, in units.
fn bake_length() -> f32 {
    return params.clock.z * SPEED + BAKE_BEFORE + BAKE_AFTER;
}

// Where `p` falls in the baked land, 0..1 inside it.
fn bake_uv(p: vec2<f32>, l: Land) -> vec2<f32> {
    return vec2<f32>(dot(p, l.side) / (2.0 * BAKE_HALF) + 0.5, (dot(p, l.forward) + BAKE_BEFORE) / bake_length());
}

fn in_bake(uv: vec2<f32>) -> bool {
    return params.clock.z > 0.0 && all(uv > vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0));
}

// Octaves above the bake's, for shading: fine bumps on the baked land.
fn fine_detail(p: vec2<f32>, octaves: i32, l: Land) -> f32 {
    var q = p * 0.2 + l.offset;
    var b = 1.0;
    for (var i = 0; i < BAKE_OCTAVES; i++) {
        q = M2 * q * 2.0;
        b *= 0.5;
    }
    var a = 0.0;
    for (var i = BAKE_OCTAVES; i < octaves; i++) {
        a += b * (world_noised(q).x - 0.5);
        b *= 0.5;
        q = M2 * q * 2.0;
    }
    return a * 0.35 * l.height;
}

// A bilinear read with the texel weights smoothstepped (Inigo Quilez's
// "improved texture filtering"): one sample, and no visible facets where
// a texel spans many pixels.
fn land_smooth(uv: vec2<f32>, lod: f32) -> f32 {
    let size = vec2<f32>(textureDimensions(height_map, i32(lod)));
    let q = uv * size + 0.5;
    let i = floor(q);
    let f = fract(q);
    let s = f * f * (3.0 - 2.0 * f);
    return textureSampleLevel(height_map, song_sampler, (i + s - 0.5) / size, lod).r;
}

// The ground's height: one texture read inside the baked land, the
// procedural land outside it (no song, or far off to the side). Up close
// (`octaves` above the bake's) the finer octaves become real geometry.
fn terrain(p: vec2<f32>, octaves: i32, l: Land) -> f32 {
    let uv = bake_uv(p, l);
    if in_bake(uv) {
        var h = land_smooth(uv, 0.0);
        if octaves > BAKE_OCTAVES {
            h += fine_detail(p, octaves, l);
        }
        return h;
    }
    return terrain_procedural(p, min(octaves, 4), l);
}

// Octaves for shading: full detail near, less with distance.
fn detail(t: f32, quality: f32) -> i32 {
    let near = select(9, 7, quality < 0.9);
    return clamp(near - i32(log2(1.0 + t * 0.25) * 1.6), 4, near);
}

// The baked land at a level of detail (0 full, each level half as fine).
fn land_at(p: vec2<f32>, lod: f32, l: Land) -> f32 {
    return land_smooth(bake_uv(p, l), floor(lod));
}

// Units per texel of the bake's full level.
fn bake_texel() -> f32 {
    return 2.0 * BAKE_HALF / f32(textureDimensions(height_map).x);
}

// The level of detail a pixel at distance `t` covers: one texel per pixel.
fn land_lod(t: f32) -> f32 {
    let pixel = t * 2.0 / (1.7 * min(params.output.x, params.output.y));
    return clamp(log2(max(pixel / bake_texel(), 1.0)), 0.0, 6.0);
}

// The ground's normal: the baked shape at the pixel's level of detail
// over about a texel of it (a smaller step shows the bilinear facets),
// plus the finer octaves' bumps, which fade out with distance.
fn ground_normal(p: vec2<f32>, t: f32, l: Land, octaves: i32) -> vec3<f32> {
    let uv = bake_uv(p, l);
    var grad: vec2<f32>;
    if in_bake(uv) {
        let lod = land_lod(t);
        let e = bake_texel() * exp2(lod);
        let hx = land_at(p + vec2<f32>(e, 0.0), lod, l) - land_at(p - vec2<f32>(e, 0.0), lod, l);
        let hz = land_at(p + vec2<f32>(0.0, e), lod, l) - land_at(p - vec2<f32>(0.0, e), lod, l);
        grad = vec2<f32>(hx, hz) / (2.0 * e);
        if octaves > BAKE_OCTAVES {
            let f = 0.0015 + 0.0012 * t;
            let d = fine_detail(p, octaves, l);
            grad += vec2<f32>(fine_detail(p + vec2<f32>(f, 0.0), octaves, l) - d, fine_detail(p + vec2<f32>(0.0, f), octaves, l) - d) / f;
        }
    } else {
        let e = 0.0015 + 0.0012 * t;
        let h = terrain(p, octaves, l);
        grad = vec2<f32>(terrain(p + vec2<f32>(e, 0.0), octaves, l) - h, terrain(p + vec2<f32>(0.0, e), octaves, l) - h) / e;
    }
    return normalize(vec3<f32>(-grad.x, 1.0, -grad.y));
}

// Ambient occlusion from the mips: ground lower than its blurred
// surroundings (valleys, gullies, crevices) sees less sky.
fn land_occlusion(p: vec3<f32>, t: f32, l: Land) -> f32 {
    let uv = bake_uv(p.xz, l);
    if !in_bake(uv) {
        return 0.8;
    }
    let base = land_lod(t);
    let h = land_at(p.xz, base, l);
    let near = h - land_at(p.xz, base + 2.0, l);
    let wide = h - land_at(p.xz, base + 4.0, l);
    let k = 1.0 / l.height;
    return clamp(1.0 + near * 3.0 * k, 0.45, 1.0) * clamp(1.0 + wide * 1.6 * k, 0.55, 1.05);
}

// Tree crowns: 0 in the gaps, 1 on a crown's top (cells about 0.05 units
// across, one tree each, jittered; some cells empty, so they clump).
fn tree_crowns(p: vec2<f32>) -> f32 {
    let q = p * 19.0;
    let i = floor(q);
    let f = fract(q);
    var d = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let c = i + o;
            let r = o + vec2<f32>(world_hash(c), world_hash(c + 17.3)) * 0.7 + 0.15 - f;
            let size = (0.6 + 0.45 * world_hash(c + 41.7)) * step(0.18, world_hash(c + 5.1));
            d = min(d, dot(r, r) / max(size * size, 1e-3));
        }
    }
    // Ragged edges: the crown's reach wobbles with a finer noise.
    let ragged = 1.45 + 0.6 * world_noised(p * 63.0).x;
    let crown = clamp(1.0 - sqrt(d) * ragged, 0.0, 1.0);
    return sqrt(crown);
}

// Where forest grows: gentle, low and middle ground, in patches; none in
// the desert.
fn forest_cover(p: vec3<f32>, slope: f32, l: Land) -> f32 {
    if l.biome == 2 {
        return 0.0;
    }
    let h01 = clamp(p.y / l.height + 0.2, 0.0, 1.0);
    let patches = smoothstep(0.42, 0.58, world_fbm(p.xz * 0.35 + 7.0, 3));
    let band = smoothstep(0.05, 0.12, h01 - max(l.water / l.height + 0.2, 0.0)) * smoothstep(0.62, 0.45, h01);
    return patches * band * smoothstep(0.42, 0.25, slope);
}

// The sun stands ahead of the flight, off to one side (side and back
// light read as depth; light from behind the camera reads flat), mostly
// low in the sky.
fn sun_dir() -> vec3<f32> {
    let side = select(-1.0, 1.0, params.seed.x > 0.5);
    let az = params.seed.w * 2.0 * PI + side * mix(0.35, 1.25, fract(params.seed.x * 7.7));
    // The song's mode sets the hour: minor keys at dusk, major ones from
    // golden hour into the day.
    let el = mix(0.035, 0.3, params.tune.w) + 0.07 * fract(params.seed.x * 13.7);
    return normalize(vec3<f32>(cos(az) * cos(el), sin(el), sin(az) * cos(el)));
}

// Natural colour moved towards the cover's hue: lightness kept, chroma
// from the cover (capped), `amount` of the way.
fn tint(natural: vec3<f32>, cover: vec3<f32>, amount: f32) -> vec3<f32> {
    let a = oklab(natural);
    let b = oklab(cover);
    let cb = length(b.yz);
    let ca = length(a.yz);
    // The cover's hue at a chroma between the natural one and the cover's.
    let hue = select(a.yz / max(ca, 1e-5), b.yz / max(cb, 1e-5), cb > 0.01);
    let chroma = mix(ca, min(max(cb, ca), 0.12), 0.7);
    let ab = mix(a.yz, hue * chroma, amount * params.tune.y);
    return max(oklab_to_linear(vec3<f32>(a.x, ab)), vec3<f32>(0.0));
}

// The frame's light and sky colours. They are the same for every pixel,
// and tint() costs a few OKLab conversions, so fs_main works them out once
// and passes them down.
struct Look {
    sun: vec3<f32>,
    // Sunlight: warmer and dimmer the lower the sun, a touch of the cover.
    light: vec3<f32>,
    zenith: vec3<f32>,
    horizon: vec3<f32>,
    // The horizon's colour towards the sun at a low sun.
    glow: vec3<f32>,
    low_sun: f32,
};

fn look() -> Look {
    var lk: Look;
    lk.sun = sun_dir();
    let warm = vec3<f32>(1.0, 0.52, 0.26);
    let noon = vec3<f32>(1.0, 0.94, 0.86);
    lk.light = tint(mix(warm, noon, smoothstep(0.05, 0.4, lk.sun.y)), pal(1), 0.18) * (2.4 + 0.3 * params.env.x);
    lk.low_sun = 1.0 - smoothstep(0.05, 0.45, lk.sun.y);
    lk.zenith = tint(vec3<f32>(0.025, 0.055, 0.17), pal(0), 0.35) * mix(1.0, 0.6, lk.low_sun);
    lk.horizon = tint(vec3<f32>(0.22, 0.27, 0.36), pal(1), 0.6);
    lk.glow = lk.light * 0.3;
    return lk;
}

fn sky(rd: vec3<f32>, lk: Look) -> vec3<f32> {
    let up = clamp(rd.y, 0.0, 1.0);
    let flat_rd = normalize(vec3<f32>(rd.x, 0.0, rd.z) + vec3<f32>(1e-4, 0.0, 0.0));
    let flat_sun = normalize(vec3<f32>(lk.sun.x, 0.0, lk.sun.z) + vec3<f32>(1e-4, 0.0, 0.0));
    let toward = pow(max(dot(flat_rd, flat_sun), 0.0), 3.0);
    let horizon = mix(lk.horizon, lk.glow, toward * lk.low_sun * 0.8);
    var col = mix(horizon, lk.zenith, pow(up, 0.35));
    // Below the horizon: the haze's colour, for the ground's fog.
    col = mix(col, horizon * 0.9, smoothstep(0.0, -0.1, rd.y));
    let s = max(dot(rd, lk.sun), 0.0);
    col += lk.light * (0.06 * pow(s, 6.0) + 0.25 * pow(s, 64.0) + 3.0 * smoothstep(0.99988, 0.99996, s));
    return col;
}

// Cloud cover at a point of the cloud plane's texture space.
fn cloud_base(q: vec2<f32>, octaves: i32) -> f32 {
    return world_fbm(q + params.seed.zw * 50.0, octaves);
}

fn cloud_uv(xz: vec2<f32>) -> vec2<f32> {
    return xz * 0.035 + vec2<f32>(params.clock.x * 0.006, params.clock.x * 0.002);
}

// Two layers of cloud on a plane at y = 14, lit from the sun; returns the
// colour and how much it covers.
fn clouds(ro: vec3<f32>, rd: vec3<f32>, lk: Look) -> vec4<f32> {
    if rd.y <= 0.02 {
        return vec4<f32>(0.0);
    }
    let t = (14.0 - ro.y) / rd.y;
    let base = cloud_base(cloud_uv(ro.xz + rd.xz * t), 5);
    let amount = mix(0.5, 0.68, params.seed.z);
    let density = smoothstep(amount, amount + 0.25, base) * smoothstep(0.02, 0.25, rd.y);
    // Thicker inside: darker; the sun's side glows.
    let thick = smoothstep(amount, amount + 0.45, base);
    let s = max(dot(rd, lk.sun), 0.0);
    let lit = mix(lk.light * 0.45, mix(lk.horizon, lk.zenith, 0.6) * 0.9, thick * 0.6)
        + lk.light * pow(s, 8.0) * 0.4 * (1.0 - thick);
    return vec4<f32>(lit, density * 0.7);
}

// Cloud shadow on the ground at `p`.
fn cloud_shadow(p: vec3<f32>, sun: vec3<f32>) -> f32 {
    let t = (14.0 - p.y) / max(sun.y, 0.05);
    let amount = mix(0.42, 0.62, params.seed.z);
    return 1.0 - 0.3 * smoothstep(amount - 0.05, amount + 0.4, cloud_base(cloud_uv(p.xz + sun.xz * t), 3));
}

// The flight: down the middle of the song's valley, `t` units along it,
// weaving gently between the tonic's walls.
fn flight(t: f32, l: Land) -> vec2<f32> {
    return l.forward * t + l.side * (sin(t * 0.09) * 1.2 + sin(t * 0.037) * 0.8);
}

// Ground colour at `p`: natural materials by slope, height and noise,
// forest over them (`canopy`: x how much forest, y the crown here, z how
// near: crowns resolve up close, their mean further away); w is the
// specular strength.
fn material(p: vec3<f32>, n: vec3<f32>, l: Land, t: f32, canopy: vec3<f32>) -> vec4<f32> {
    let slope = 1.0 - n.y;
    let h01 = clamp(p.y / l.height + 0.2, 0.0, 1.0);
    let music = song_at(p.xz, l);
    // Ground texture from above; rock from three sides (triplanar), so
    // steep faces don't streak.
    let uv = p.xz;
    let breakup = world_fbm(uv * 1.7, 3);
    let fine = world_noised(uv * 11.0).x;
    let w = pow(abs(n), vec3<f32>(4.0));
    let wn = w / (w.x + w.y + w.z);
    let rock_grain = world_fbm(p.zy * vec2<f32>(2.2, 6.0), 3) * wn.x + breakup * wn.y
        + world_fbm(p.xy * vec2<f32>(2.2, 6.0) + 31.0, 3) * wn.z;
    let rock = mix(vec3<f32>(0.09, 0.085, 0.078), vec3<f32>(0.22, 0.2, 0.17), rock_grain);
    // Strata across the rock.
    let strata = 0.85 + 0.15 * sin(p.y * 9.0 + rock_grain * 4.0);
    var col = rock * strata;
    var spec = 0.04;
    switch l.biome {
        case 0: {
            let grass = mix(vec3<f32>(0.04, 0.075, 0.025), vec3<f32>(0.13, 0.12, 0.05), breakup * breakup);
            col = mix(grass, col, smoothstep(0.32, 0.55, slope + 0.15 * (fine - 0.5)));
        }
        case 1: {
            let low = mix(vec3<f32>(0.03, 0.055, 0.025), vec3<f32>(0.09, 0.085, 0.05), breakup);
            col = mix(low, col, smoothstep(0.08, 0.35, h01 + slope * 0.6 + 0.1 * (fine - 0.5)));
        }
        case 2: {
            let sand = mix(vec3<f32>(0.42, 0.28, 0.15), vec3<f32>(0.55, 0.4, 0.24), breakup);
            let red_rock = mix(vec3<f32>(0.3, 0.14, 0.07), vec3<f32>(0.42, 0.24, 0.12), breakup) * strata;
            col = mix(sand, red_rock, smoothstep(0.2, 0.42, slope + 0.1 * (fine - 0.5)));
        }
        default: {
            let beach = smoothstep(l.water + 0.25, l.water + 0.06, p.y);
            let green = mix(vec3<f32>(0.035, 0.07, 0.03), vec3<f32>(0.09, 0.1, 0.04), breakup);
            col = mix(mix(green, col, smoothstep(0.35, 0.6, slope)), vec3<f32>(0.5, 0.42, 0.3), beach);
        }
    }
    // Snow on high, flatter ground where the music is bright (none in the
    // desert); its edge broken up by noise.
    if l.biome != 2 {
        let snow_line = mix(0.95, 0.55, music.g) + 0.12 * breakup - 0.1 * n.y - select(0.0, 0.1, l.biome == 1);
        let snow = smoothstep(snow_line, snow_line + 0.05, h01) * smoothstep(0.42, 0.22, slope);
        col = mix(col, vec3<f32>(0.78, 0.8, 0.84), snow);
        spec = mix(spec, 0.25, snow);
    }
    // Fine grain only on gentle ground; on steep faces it aliases. The
    // finest only up close, where it resolves.
    col *= 1.0 + 0.3 * (fine - 0.5) * (1.0 - smoothstep(0.2, 0.6, slope));
    let close = 1.0 - smoothstep(3.0, 12.0, t);
    if close > 0.0 {
        let micro = world_noised(uv * 43.0).x * 0.6 + world_noised(uv * 97.0).x * 0.4;
        col *= 1.0 + 0.35 * (micro - 0.5) * close;
    }
    // Forest: dark conifer green, lit crowns over shaded gaps up close.
    if canopy.x > 0.0 {
        let leaves = mix(vec3<f32>(0.016, 0.032, 0.014), vec3<f32>(0.03, 0.05, 0.02), breakup);
        let crowns = mix(leaves * 0.75, leaves * mix(0.55, 1.25, canopy.y), canopy.z);
        col = mix(col, crowns, canopy.x);
        spec = mix(spec, 0.02, canopy.x);
    }
    // Hue from the cover's lower half, more on the flat ground.
    let cover = mix(pal(3), pal(2), smoothstep(0.2, 0.6, slope));
    return vec4<f32>(tint(col, cover, 0.22), spec);
}

// Aerial perspective: fog thicker low down (valley mist), lit by the sun
// on its side, in the sky's horizon colour.
fn atmosphere(col: vec3<f32>, ro: vec3<f32>, rd: vec3<f32>, t: f32, lk: Look, l: Land, backdrop: bool) -> vec3<f32> {
    let density = mix(0.012, 0.03, params.seed.z) * select(1.0, 1.45, backdrop);
    let falloff = 1.3 / l.height;
    let y0 = ro.y + l.height * 0.2;
    let k = rd.y * falloff;
    // Integral of exp(-falloff * height) along the ray; the straight
    // distance when the ray is level.
    let path = select(t, (1.0 - exp(-t * k)) / k, abs(k) > 1e-4);
    let fog = clamp(density * exp(-y0 * falloff) * path, 0.0, 1.0);
    let s = pow(max(dot(rd, lk.sun), 0.0), 8.0);
    let haze = sky(vec3<f32>(rd.x, 0.0, rd.z), lk) * 0.85 + lk.light * s * 0.15;
    // Distance alone also blues things out a little.
    let far = 0.85 * (1.0 - exp(-t * 0.007));
    return mix(col, haze, max(fog, far));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let frag = in.position.xy;
    let l = land();
    let lk = look();
    let sun = lk.sun;
    let backdrop = params.output.w < 0.5;
    let quality = params.tune.z;

    // Camera: high over the land, kept above the ground ahead, looking
    // slightly down into the distance. Where playback is, a little behind
    // so the present is just ahead.
    let ft = params.clock.y * SPEED - 3.0;
    let cam2 = flight(ft, l);
    let ahead2 = flight(ft + 4.0, l);
    var ground = l.water;
    for (var k = 0; k < 3; k++) {
        ground = max(ground, terrain(flight(ft + f32(k) * 3.0, l), 3, l));
    }
    let clearance = l.height * 0.45;
    let ro = vec3<f32>(cam2.x, ground + clearance + 0.3 * sin(ft * 0.13), cam2.y);
    let aim = vec3<f32>(ahead2.x, ro.y - clearance * 0.45, ahead2.y);
    let fw = normalize(aim - ro);
    let rt = normalize(cross(fw, vec3<f32>(0.0, 1.0, 0.0)));
    let up = cross(rt, fw);
    let s2 = rotate2(sin(ft * 0.07) * 0.035) * screen(frag);
    let rd = normalize(fw * 1.7 + rt * s2.x + up * s2.y);

    // March the height field: coarse octaves to find the ground (the
    // detail is in the shading), steps that grow with distance.
    let steps = i32(96.0 * quality);
    let march_oct = select(4, 3, quality < 0.9);
    var t = 0.1;
    var hit = false;
    for (var i = 0; i < steps; i++) {
        let p = ro + rd * t;
        let h = p.y - terrain(p.xz, select(march_oct, 8, t < 7.0), l);
        if h < 0.002 * t {
            hit = true;
            break;
        }
        if t > FAR || (rd.y > 0.0 && p.y > l.height * 2.0) {
            break;
        }
        t += 0.5 * h + 0.004 * t;
    }
    // Refine the hit between the last two steps.
    if hit {
        var lo = max(t - 0.6, 0.05);
        var hi = t;
        for (var k = 0; k < 3; k++) {
            let mid = 0.5 * (lo + hi);
            let pm = ro + rd * mid;
            if pm.y - terrain(pm.xz, select(march_oct, 8, mid < 7.0), l) < 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        t = hi;
    }

    // Water in front of the ground.
    var water_hit = false;
    var shore = 0.0;
    if rd.y < 0.0 {
        let tw = (l.water - ro.y) / rd.y;
        if tw > 0.0 && (!hit || tw < t) {
            let pw = ro + rd * tw;
            shore = clamp(l.water - terrain(pw.xz, 3, l), 0.0, 1.0);
            water_hit = true;
            t = tw;
            hit = true;
        }
    }

    var col: vec3<f32>;
    if !hit {
        col = sky(rd, lk);
        let c = clouds(ro, rd, lk);
        col = mix(col, c.rgb, c.a);
    } else {
        let p = ro + rd * t;
        let sl = lk.light;
        let sky_up = mix(lk.horizon, lk.zenith, 0.7);
        if water_hit {
            // Ripples: the highs stir the surface, slowly.
            let wp = p.xz * 2.2 + vec2<f32>(params.clock.x * 0.18, params.clock.x * 0.05);
            let ripple = (world_noised(wp).yz + 0.5 * world_noised(wp * 2.7 + 3.0).yz) * (0.03 + 0.05 * params.env.z);
            let n = normalize(vec3<f32>(ripple.x, 1.0, ripple.y));
            let r = reflect(rd, n);
            // A slightly rough surface: never a mirror, so the low sun
            // doesn't smear white across it.
            let fres = 0.03 + 0.55 * pow(1.0 - max(dot(-rd, n), 0.0), 5.0);
            let depth = smoothstep(0.0, 0.6, shore);
            let water = mix(vec3<f32>(0.07, 0.13, 0.12), vec3<f32>(0.03, 0.065, 0.085), depth);
            let body = tint(water, pal(2), 0.4) * (0.35 * sl + sky_up * 0.9);
            var refl = mix(lk.horizon, lk.zenith, pow(clamp(r.y, 0.0, 1.0), 0.35));
            let c = clouds(p, r, lk);
            refl = mix(refl, c.rgb, c.a);
            col = mix(body, refl, fres);
            // A soft, narrow glitter path under the sun.
            col += sl * 0.25 * pow(max(dot(r, sun), 0.0), 900.0) * (0.6 + 0.4 * params.env.z) * cloud_shadow(p, sun);
            // Shallow edge foam, faint.
            col += vec3<f32>(0.2) * smoothstep(0.06, 0.0, shore) * (0.5 + 0.5 * world_noised(p.xz * 6.0 + params.clock.x * 0.2).x) * 0.3;
        } else {
            let oct = detail(t, quality);
            var n = ground_normal(p.xz, t, l, oct);
            // Forest: crowns bump the normal and shade their gaps up
            // close; further away only their mean shows.
            let forest = forest_cover(p, 1.0 - n.y, l);
            let near = 1.0 - smoothstep(2.5, 7.0, t);
            var crown = 0.6;
            var tree_ao = 1.0;
            if forest > 0.01 && near > 0.0 {
                crown = tree_crowns(p.xz);
                let e = 0.005;
                let g = vec2<f32>(tree_crowns(p.xz + vec2<f32>(e, 0.0)) - crown, tree_crowns(p.xz + vec2<f32>(0.0, e)) - crown) / e;
                n = normalize(n + vec3<f32>(-g.x, 0.0, -g.y) * 0.006 * forest * near);
                tree_ao = mix(1.0, mix(0.55, 1.0, crown), forest * near);
            }
            tree_ao *= mix(1.0, 0.75, forest * (1.0 - near));
            let m = material(p, n, l, t, vec3<f32>(forest, crown, near));
            // Occlusion: valleys and gullies from the land's mips, the
            // finer cavities from the detail octaves, the forest's gaps.
            let cavity = land_occlusion(p, t, l)
                * clamp(0.85 + fine_detail(p.xz, oct, l) * 5.0 / l.height, 0.5, 1.0) * tree_ao;
            // Soft shadow towards the sun.
            var shadow = 1.0;
            var st = 0.08 + 0.003 * t;
            let shadow_steps = i32(8.0 * quality);
            for (var k = 0; k < shadow_steps; k++) {
                let sp3 = p + sun * st;
                let sh = sp3.y - terrain(sp3.xz, 3, l);
                shadow = min(shadow, 10.0 * sh / st);
                if shadow < 0.0 || sp3.y > l.height * 2.0 {
                    break;
                }
                st += clamp(sh, 0.15, 1.5);
            }
            shadow = smoothstep(0.0, 1.0, clamp(shadow, 0.0, 1.0)) * cloud_shadow(p, sun);
            let dif = max(dot(n, sun), 0.0);
            let amb = sky_up * (0.25 + 0.3 * n.y) * cavity * mix(0.6, 1.0, shadow);
            let bounce = m.rgb * sl * 0.12 * clamp(1.0 - n.y, 0.0, 1.0) * cavity;
            let h = normalize(sun - rd);
            let spec = m.a * pow(max(dot(n, h), 0.0), 24.0) * dif * shadow;
            col = m.rgb * (sl * dif * shadow + amb) + bounce + sl * spec;
        }
        col = atmosphere(col, ro, rd, t, lk, l, backdrop);
    }

    // Strength: how far the scene stands out from its own haze.
    if abs(params.tune.x - 1.0) > 0.01 {
        col = mix(sky(vec3<f32>(rd.x, 0.02, rd.z), lk), col, clamp(params.tune.x, 0.0, 1.5));
    }
    let v = in.uv - 0.5;
    col *= 1.0 - 0.4 * dot(v, v) * select(1.0, 1.6, backdrop);
    return finish(col, frag);
}

// The bake: the land along the whole song written into a height texture
// once per song (r16float), so the march reads one texel a step instead
// of evaluating the song map and BAKE_OCTAVES of noise.
@fragment
fn fs_bake(in: VertexOut) -> @location(0) vec4<f32> {
    let l = land();
    let along = in.uv.y * bake_length() - BAKE_BEFORE;
    let across = (in.uv.x - 0.5) * 2.0 * BAKE_HALF;
    let p = l.forward * along + l.side * across;
    return vec4<f32>(terrain_procedural(p, BAKE_OCTAVES, l), 0.0, 0.0, 1.0);
}
