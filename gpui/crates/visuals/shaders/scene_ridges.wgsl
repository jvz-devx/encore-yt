// Concept 4, flyover with spectrum ridges: a slow flight low over dark land
// whose ridges are the spectrum, Unknown Pleasures meets a calm sunset.
// Across the width the 32 bands mirror out from a central spine (bass in
// the middle, highs on the flanks, plain beyond); along the depth the land
// rises into a ridge every few metres, each row a slowly varied, blurred
// copy of the current bands, so the land flows towards the camera. A fine
// line glints along every ridge top; the ridges recede into haze towards a
// glowing horizon in the cover's colours.
//
// Seeds: x the ridge spacing, y the land noise and how the rows wander, z the haze,
// w the sun's place on the horizon.

struct RidgeLook {
    // Height of the spectrum ridges.
    amp: f32,
    // Distance between two ridge tops along the flight.
    spacing: f32,
    // How much the band lookups are blurred (backdrop: soft and calm).
    blur: f32,
    // The spine's x (the bass), where the camera does not fly.
    spine: f32,
    noise_offset: vec2<f32>,
    // The spectrum as cosine coefficients (see ridge_spectrum): 12 as
    // sharp as this mode allows (s0..s2) and 8 fully blurred (b0, b1).
    s0: vec4<f32>,
    s1: vec4<f32>,
    s2: vec4<f32>,
    b0: vec4<f32>,
    b1: vec4<f32>,
};

const RIDGE_HALF_WIDTH: f32 = 5.0;

fn ridge_look() -> RidgeLook {
    let vis = params.output.w;
    var r: RidgeLook;
    r.amp = mix(0.6, 1.25, vis) * (0.85 + 0.3 * params.env.w);
    r.spacing = mix(0.7, 0.95, params.seed.x);
    r.blur = mix(0.7, 0.25, vis);
    r.spine = 0.0;
    r.noise_offset = vec2<f32>(params.seed.y * 571.0, fract(params.seed.y * 9.13) * 379.0);
    ridge_spectrum(&r);
    return r;
}

// A slow, smooth wave along the flight, 0..1: a beating pair of sines,
// much cheaper than hashed noise inside the march.
fn ridge_wave(x: f32, phase: f32) -> f32 {
    return 0.5 + 0.5 * sin(x + phase) * cos(x * 0.37 + phase * 1.7);
}

// Value noise, 0..1.
fn ridge_vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The 32 bands as a cosine series (a DCT-II, 12 terms), once per pixel.
// Inside the march the spectrum is then a short recurrence with no
// indexing into the uniform array. Cosines are even about 0 and 1, so the
// mirrored spine and the far edge come out smooth for free, and 12 terms
// keep features about three bands wide: land, not a bar graph.
const RIDGE_K2_0: vec4<f32> = vec4<f32>(0.0, 1.0, 4.0, 9.0);
const RIDGE_K2_1: vec4<f32> = vec4<f32>(16.0, 25.0, 36.0, 49.0);
const RIDGE_K2_2: vec4<f32> = vec4<f32>(64.0, 81.0, 100.0, 121.0);

fn ridge_spectrum(r: ptr<function, RidgeLook>) {
    var a0 = vec4<f32>(0.0);
    var a1 = vec4<f32>(0.0);
    var a2 = vec4<f32>(0.0);
    for (var j = 0; j < 8; j++) {
        let b = params.bands[j];
        let x = (vec4<f32>(f32(j * 4)) + vec4<f32>(0.5, 1.5, 2.5, 3.5)) / 32.0;
        let c = cos(PI * x);
        // Chebyshev recurrence: t_k = cos(k pi x).
        let t2 = 2.0 * c * c - vec4<f32>(1.0);
        let t3 = 2.0 * c * t2 - c;
        let t4 = 2.0 * c * t3 - t2;
        let t5 = 2.0 * c * t4 - t3;
        let t6 = 2.0 * c * t5 - t4;
        let t7 = 2.0 * c * t6 - t5;
        let t8 = 2.0 * c * t7 - t6;
        let t9 = 2.0 * c * t8 - t7;
        let t10 = 2.0 * c * t9 - t8;
        let t11 = 2.0 * c * t10 - t9;
        a0 += vec4<f32>(dot(b, vec4<f32>(1.0)), dot(b, c), dot(b, t2), dot(b, t3));
        a1 += vec4<f32>(dot(b, t4), dot(b, t5), dot(b, t6), dot(b, t7));
        a2 += vec4<f32>(dot(b, t8), dot(b, t9), dot(b, t10), dot(b, t11));
    }
    a0 *= vec4<f32>(1.0, 2.0, 2.0, 2.0) / 32.0;
    a1 *= 2.0 / 32.0;
    a2 *= 2.0 / 32.0;
    // A gentle low-pass even when sharp, against ringing.
    let sharp = 0.006 + 0.06 * (*r).blur;
    (*r).s0 = a0 / (vec4<f32>(1.0) + sharp * RIDGE_K2_0);
    (*r).s1 = a1 / (vec4<f32>(1.0) + sharp * RIDGE_K2_1);
    (*r).s2 = a2 / (vec4<f32>(1.0) + sharp * RIDGE_K2_2);
    (*r).b0 = a0 / (vec4<f32>(1.0) + 0.16 * RIDGE_K2_0);
    (*r).b1 = a1 / (vec4<f32>(1.0) + 0.16 * RIDGE_K2_1);
}

// The spectrum at `x` (0 bass .. 1 highs), from the mode's sharpest (blur
// 0) to its softest (blur 1).
fn ridge_bands(x: f32, blur: f32, r: RidgeLook) -> f32 {
    let c = cos(PI * clamp(x, 0.0, 1.0));
    let t2 = 2.0 * c * c - 1.0;
    let t3 = 2.0 * c * t2 - c;
    let t4 = 2.0 * c * t3 - t2;
    let t5 = 2.0 * c * t4 - t3;
    let t6 = 2.0 * c * t5 - t4;
    let t7 = 2.0 * c * t6 - t5;
    let t8 = 2.0 * c * t7 - t6;
    let t9 = 2.0 * c * t8 - t7;
    let t10 = 2.0 * c * t9 - t8;
    let t11 = 2.0 * c * t10 - t9;
    let lo = vec4<f32>(1.0, c, t2, t3);
    let hi = vec4<f32>(t4, t5, t6, t7);
    let sharp = dot(r.s0, lo) + dot(r.s1, hi) + dot(r.s2, vec4<f32>(t8, t9, t10, t11));
    let soft = dot(r.b0, lo) + dot(r.b1, hi);
    return mix(sharp, soft, blur);
}

// How far the rows wander along the flight across the width (in rows),
// so they read as land and not as straight furrows.
fn ridge_warp(x: f32) -> f32 {
    let ph = params.seed.y * 6.3;
    return 0.22 * sin(x * 0.55 + ph) + 0.1 * sin(x * 1.37 + ph * 2.1 + 1.0);
}

// The land at `p`: x height, y the row coordinate (ridge tops at whole
// numbers), z the bands' value there, w the position across 0 (spine)
// to 1 (the highs' edge) and beyond.
// `fine` adds a finer octave of land: off in the march, on for shading.
fn ridge_sample(p: vec2<f32>, cam_z: f32, r: RidgeLook, fine: bool) -> vec4<f32> {
    // One octave of land noise plus, for shading, a finer, weaker one.
    let q = p * 0.33 + r.noise_offset;
    let n = ridge_vnoise(q);
    var n2 = 0.5;
    if fine {
        n2 = ridge_vnoise(q * 2.7 + vec2<f32>(13.1, 7.7));
    }

    // A soft abs, so the spine is a rounded crest and not a crease.
    let dx = p.x - r.spine;
    let across = (sqrt(dx * dx + 0.36) - 0.6) / RIDGE_HALF_WIDTH;
    // The rows wander a little across the width, so they read as land.
    let row = p.y / r.spacing + ridge_warp(p.x);
    // Each row is a slow variation of the current bands: lower, shifted
    // and blurrier the further off it is, and from row to row.
    let zr = p.y / r.spacing;
    let row_amp = 0.55 + 0.45 * ridge_wave(zr * 0.9, params.seed.y * 6.3);
    let shift = (ridge_wave(zr * 0.55, params.seed.x * 6.3 + 2.0) - 0.5) * 0.16;
    let dist = abs(p.y - cam_z);
    let age = smoothstep(4.0, 26.0, dist);
    let blur = clamp(0.7 * age + 0.3 * ridge_wave(zr * 0.6, 4.0), 0.0, 1.0);
    let at = across * (1.15 - 0.15 * across) + shift;
    let b = max(ridge_bands(at, blur, r), 0.0);
    // The spectrum fades into a plain at the edges; the middle stands taller.
    let envelope = (1.0 - 0.45 * across) * (1.0 - smoothstep(0.55, 1.08, across));
    // A ridge every row.
    let c = 1.0 - 2.0 * abs(fract(row + 0.5) - 0.5);
    // Half linear, so the top stays on the row line where the land tilts.
    let crest = 0.5 * c + 0.5 * c * c * (3.0 - 2.0 * c);
    let ridge = r.amp * b * envelope * row_amp * (1.0 - 0.45 * age) * (0.22 + 0.78 * crest);
    let land = (n - 0.5) * 0.3 + (n2 - 0.5) * 0.1;
    return vec4<f32>(ridge + land, row, b, across);
}

fn ridge_h(p: vec2<f32>, cam_z: f32, r: RidgeLook, fine: bool) -> f32 {
    return ridge_sample(p, cam_z, r, fine).x;
}

fn ridge_sun() -> vec3<f32> {
    let backdrop = params.output.w < 0.5;
    // Backdrop: the sun sits centre-right, away from the cover and text.
    let x = select(0.08, 0.32, backdrop) + (params.seed.w - 0.5) * 0.24;
    return normalize(vec3<f32>(x, 0.045, -1.0));
}

fn ridge_horizon() -> vec3<f32> {
    return mix(pal(1), pal(0), 0.3);
}

fn ridge_sky(rd: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    let up = max(rd.y, 0.0);
    // Dark above, the cover's cooler colours; the glow hugs the horizon.
    let zenith = mix(pal(3), pal(2), 0.5) * 0.06;
    let glow = exp(-up * 7.0);
    var col = mix(zenith, ridge_horizon() * 0.42, glow);
    let s = max(dot(rd, sun), 0.0);
    // A low, soft sun in the cover's warm colour, never white.
    let core = mix(pal(1), vec3<f32>(1.0, 0.88, 0.75), 0.2);
    col += core * (0.28 * pow(s, 12.0) + 0.35 * pow(s, 120.0));
    // A thin bright line right on the horizon, strongest towards the sun.
    col += ridge_horizon() * exp(-up * 60.0) * (0.08 + 0.25 * pow(s, 6.0));
    return col;
}

// The colour where the ray meets the land at `t`: dark land lit from the
// low sun ahead, the line along the ridge top, and the haze.
fn ridge_shade(ro: vec3<f32>, rd: vec3<f32>, t: f32, cam_z: f32, r: RidgeLook, sun: vec3<f32>,
    haze_col: vec3<f32>, pixel: f32, far: f32) -> vec3<f32> {
    let backdrop = params.output.w < 0.5;
    let vis = params.output.w;
    let p = ro + rd * t;
    let s = ridge_sample(p.xz, cam_z, r, true);
    let e = 0.012 + 0.002 * t;
    let hx = ridge_h(p.xz + vec2<f32>(e, 0.0), cam_z, r, true);
    let hz = ridge_h(p.xz + vec2<f32>(0.0, e), cam_z, r, true);
    let n = normalize(vec3<f32>(s.x - hx, e, s.x - hz));

    // Dark land, lit from the low sun ahead: the faces we see are in
    // shade, the ridge tops catch a rim of the horizon's light.
    let albedo = mix(pal(3), pal(0), 0.25 * (1.0 - smoothstep(0.0, 1.0, s.w))) * 0.12;
    let dif = max(dot(n, sun), 0.0);
    let amb = mix(pal(2), pal(3), 0.5) * (0.08 + 0.12 * n.y);
    let ndv = max(dot(n, -rd), 0.0);
    let rim = pow(1.0 - ndv, 4.0) * ridge_horizon() * 0.1;
    var col = albedo * (amb + dif * pal(1) * 0.8) + rim;

    // The line on the ridge top: a gaussian whose width grows with the
    // pixel's footprint, dimmed as it widens, so far lines fade out
    // instead of aliasing. Centred a little on the camera's side of the
    // top, where the silhouette's blend meets it.
    let foot = t * pixel / max(abs(dot(n, rd)), 0.12);
    let lw = mix(0.012, 0.009, vis);
    let w = sqrt(lw * lw + foot * foot);
    let d = (fract(s.y + 0.5) - 0.5) * r.spacing - 0.4 * w;
    let line = exp(-(d * d) / (w * w)) * (lw / w) * (1.0 - smoothstep(5.0, far * 0.7, t));
    let halo = exp(-abs(d) / 0.09) * 0.12;
    let toward = pow(max(dot(normalize(vec3<f32>(rd.x, 0.0, rd.z)), sun), 0.0), 6.0);
    let tint = mix(mix(pal(1), pal(2), 0.5 * smoothstep(0.2, 1.1, s.w)), vec3<f32>(1.0, 0.92, 0.85), 0.2);
    let spread = 0.5 + 0.5 * (1.0 - smoothstep(0.4, 1.2, s.w));
    let glint = (0.25 + 1.1 * s.z) * spread * (0.7 + 0.6 * toward) * mix(1.0, 1.6, vis);
    col += tint * (line + halo) * glint;

    // Aerial haze into the horizon's glow, all haze at the far end.
    let density = mix(0.035, 0.06, params.seed.z) * select(1.0, 1.5, backdrop);
    let fog = max(1.0 - exp(-t * density), smoothstep(far * 0.55, far, t));
    // Near haze is dimmer than the glow it fades into.
    let haze = haze_col * mix(0.35, 1.0, smoothstep(4.0, far * 0.8, t));
    return mix(col, haze, fog);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let frag = in.position.xy;
    let r = ridge_look();
    let sun = ridge_sun();
    let backdrop = params.output.w < 0.5;
    let vis = params.output.w;
    let quality = params.tune.z;

    // Camera: forward along -z at a calm pace, a slow sway sideways. The
    // backdrop flies left of the spine so the range sits centre-right.
    let ft = params.clock.x;
    let cam_z = -ft * 0.22 - 30.0;
    let cam_x = select(0.0, -1.7, backdrop) + 0.35 * sin(ft * 0.043 + params.seed.y * 6.0);
    let cam_y = mix(1.05, 1.9, vis) + 0.08 * sin(ft * 0.061);
    let ro = vec3<f32>(cam_x, cam_y, cam_z);
    let yaw = select(0.0, 0.1, backdrop) + 0.03 * sin(ft * 0.029);
    let pitch = mix(-0.1, -0.16, vis);
    let fw = normalize(vec3<f32>(sin(yaw), pitch, -cos(yaw)));
    let rt = normalize(cross(fw, vec3<f32>(0.0, 1.0, 0.0)));
    let up = cross(rt, fw);
    let focal = 1.6;
    let sp = rotate2(0.012 * sin(ft * 0.05)) * screen(frag);
    let rd = normalize(fw * focal + rt * sp.x + up * sp.y);
    let pixel = 2.0 / (min(params.output.x, params.output.y) * focal);

    // March the height field from the plane over the tallest ridge.
    // The loudest band bounds the land: no ridge is taller than it.
    var loudest = vec4<f32>(0.0);
    for (var j = 0; j < 8; j++) {
        loudest = max(loudest, params.bands[j]);
    }
    let bmax = max(max(loudest.x, loudest.y), max(loudest.z, loudest.w));
    let top = r.amp * bmax * 1.08 + 0.24;
    let far = mix(24.0, 36.0, vis);
    var t = 0.05;
    if ro.y > top {
        t = select(far + 1.0, (ro.y - top) / max(-rd.y, 1e-4), rd.y < 0.0);
    }
    // Hop from ridge top to ridge top: between two tops the land is a
    // valley, so the first top the ray passes under brackets the hit, and
    // no thin crest is ever stepped over. Far off (in the haze) the hops
    // skip rows.
    let steps = i32(f32(28) * quality);
    let hop = mix(0.12, 0.08, vis);
    var hit = false;
    let p0 = ro + rd * t;
    var prev_t = t;
    var h = p0.y - ridge_h(p0.xz, cam_z, r, false);
    var prev_h = h;
    hit = h < 0.0;
    var row = floor(p0.z / r.spacing + ridge_warp(p0.x));
    var x_at = p0.x;
    // The crest the ray passes closest over, as a share of a pixel (1 on
    // it, 0 a pixel or more above it), and where.
    var graze = 0.0;
    var graze_t = 0.0;
    for (var i = 0; i < steps; i++) {
        if hit || t > far {
            break;
        }
        // The next ridge top, its wander taken at the last sample's x.
        let zc = (row - ridge_warp(x_at)) * r.spacing;
        t = max((zc - ro.z) / rd.z, t + 0.01);
        let p = ro + rd * t;
        if p.y > top && rd.y >= 0.0 {
            break;
        }
        h = p.y - ridge_h(p.xz, cam_z, r, false);
        if h < 0.0 {
            hit = true;
            break;
        }
        // Samples are on the ridge tops: how far above this one the ray
        // passes, in pixels at this distance.
        let cover = 1.0 - h / (t * pixel * 1.5);
        if cover > graze {
            graze = cover;
            graze_t = t;
        }
        prev_t = t;
        prev_h = h;
        x_at = p.x;
        row -= 1.0 + floor(t * hop);
    }
    if hit {
        // Four bisection steps, then a secant inside the bracket.
        var lo = prev_t;
        var hi = t;
        var h_lo = prev_h;
        var h_hi = h;
        for (var j = 0; j < 4; j++) {
            let mid = 0.5 * (lo + hi);
            let pm = ro + rd * mid;
            let hm = pm.y - ridge_h(pm.xz, cam_z, r, false);
            if hm < 0.0 {
                hi = mid;
                h_hi = hm;
            } else {
                lo = mid;
                h_lo = hm;
            }
        }
        t = lo + (hi - lo) * h_lo / max(h_lo - h_hi, 1e-5);
    }
    // Rays that ran out of steps low over the land are in the haze anyway.
    if !hit && rd.y < 0.0 && t <= far {
        hit = true;
    }

    let sky_col = ridge_sky(rd, sun);
    let haze_col = ridge_sky(normalize(vec3<f32>(rd.x, 0.0, rd.z)), sun);
    var col = sky_col;
    if hit {
        col = ridge_shade(ro, rd, t, cam_z, r, sun, haze_col, pixel, far);
    }
    // A crest the ray passed within a pixel of covers that much of the
    // pixel: blended in, its silhouette is anti-aliased (on the side of
    // what lies beyond; the crest's own side is the hit itself).
    if graze > 0.0 {
        let crest = ridge_shade(ro, rd, graze_t, cam_z, r, sun, haze_col, pixel, far);
        col = mix(col, crest, graze);
    }

    // Strength: how far the scene stands out from its own haze.
    col = mix(haze_col, col, clamp(params.tune.x, 0.0, 1.5));
    // A vignette; the backdrop keeps its left side (cover, text) quieter.
    let v = in.uv - 0.5;
    col *= 1.0 - 0.45 * dot(v, v) * select(1.0, 1.6, backdrop);
    if backdrop {
        col *= mix(0.8, 1.0, smoothstep(0.0, 0.55, in.uv.x));
    }
    return finish(col, frag);
}
