// Aurora: soft curtains of light folding slowly in a night sky over a
// dark horizon, a still lake that mirrors them or a snow plain they light.
// Each curtain is a thin vertical sheet whose footprint is a folded curve;
// a ray is sampled through the slab round that curve and the sheet's soft
// Gaussian profile is integrated analytically between samples, so it stays
// smooth at any step count and grows brighter where a fold turns edge-on.
//
// Seeds: x where the curtains hang and how they turn, y which cover
// colours they take, z lake or snow and the hills, w the folds' phase.
// Music: the slow envelopes breathe the curtains' brightness and folding,
// the highs shimmer the fine rays; the visualiser lets the bands set each
// curtain's height along its length.

const AUR_FAR: f32 = 70.0;
const AUR_CAM_H: f32 = 0.35;
const AUR_FOCAL: f32 = 1.5;

struct AurCurtain {
    centre: vec2<f32>,
    // Unit vector along the curtain, in xz.
    axis: vec2<f32>,
    half_len: f32,
    amp: f32,
    phase: f32,
    base: f32,
    height: f32,
    gain: f32,
    // The sheet's Gaussian thickness: thicker further off, so fold
    // silhouettes keep a soft edge on screen.
    w: f32,
    low: vec3<f32>,
    high: vec3<f32>,
    salt: i32,
};

// 1D value noise on the sine-free hash: the salts alone put a sine hash's
// argument above 1e4, where NVIDIA's and AMD's fast sin aren't precise.
fn aur_h(i: f32, salt: i32) -> f32 {
    return hash21(vec2<f32>(i, f32(salt)));
}

fn aur_vn(x: f32, salt: i32) -> f32 {
    let i = floor(x);
    let f = x - i;
    let u = f * f * (3.0 - 2.0 * f);
    return mix(aur_h(i, salt), aur_h(i + 1.0, salt), u);
}

fn aur_erf(x: f32) -> f32 {
    let c = clamp(x, -4.0, 4.0);
    return tanh(c * (1.128379 + 0.1009 * c * c));
}

// A palette colour as light: full value, so dark covers still glow.
fn aur_light(i: i32) -> vec3<f32> {
    let c = pal(((i % 4) + 4) % 4);
    let m = max(c.r, max(c.g, c.b));
    return c / max(m, 0.04);
}

fn aur_make(i: i32) -> AurCurtain {
    let s = params.seed;
    let backdrop = params.output.w < 0.5;
    let fi = f32(i);
    var c: AurCurtain;
    let r0 = fract(s.x * 7.31 + fi * 0.37);
    let r1 = fract(s.x * 17.3 + fi * 0.61);
    // Bearing and distance of the curtain's middle: right of centre for
    // the backdrop (the cover and text sit left), centred for the
    // visualiser; further curtains hang lower and fainter.
    var az = select(mix(-0.1, 0.1, r0), mix(0.22, 0.42, r0), backdrop);
    az += fi * mix(-0.18, 0.14, r1);
    let dist = 8.0 + fi * 6.5;
    c.centre = vec2<f32>(sin(az), cos(az)) * dist;
    // The curtain runs across the view, one end swinging near and high
    // on the right, the other receding to the horizon.
    let side = select(1.0, -1.0, fract(s.x * 31.7 + fi * 0.5) < 0.3 && !backdrop);
    // Kept well off end-on: a curtain seen along its length aliases.
    // (An axis of angle -az runs straight across the line of sight.)
    let turn = -az - side * mix(0.2, 0.5, r1) * (1.0 - 0.3 * fi);
    c.axis = vec2<f32>(cos(turn), sin(turn));
    c.half_len = 9.0 + fi * 5.0;
    // Far curtains fold less: seen low in the sky, deep folds turn their
    // lower edge into steps.
    c.amp = 1.7 - fi * 0.45;
    c.phase = s.w * 6.2831 + fi * 2.1;
    // High enough that the folded footprint shows in perspective.
    c.base = 4.2 + fi * 0.6;
    c.height = 1.1 + fi * 0.5;
    c.gain = 1.0 / (1.0 + fi * 1.1);
    c.w = 0.4 + fi * 0.4;
    // Two neighbouring cover colours per curtain, the upper one blended
    // halfway back, so a vivid cover gives a pair, not a rainbow.
    let first = i32(floor(s.y * 4.0)) + i;
    c.low = aur_light(first);
    c.high = mix(aur_light(first + 1), c.low, 0.45);
    c.salt = 101 + i * 17;
    return c;
}

// How far the folded sheet lies sideways at `u` along the curtain. The
// level's envelope nudges the phases, so the folding quickens a little
// while the music swells, and the bass deepens the folds.
fn aur_fold(u: f32, c: AurCurtain, t: f32) -> f32 {
    let p = c.phase;
    let e = params.env;
    let a = c.amp * (0.9 + 0.2 * e.x);
    return a * (0.9 * sin(0.42 * u + p + t * 0.09 + 0.6 * e.w)
        + 0.36 * sin(1.07 * u - 1.7 * p - t * 0.13 - 0.4 * e.w)
        + 0.07 * sin(2.3 * u + 2.9 * p + t * 0.21 + 0.3 * e.x));
}

// The light one curtain sends along a ray.
fn aur_curtain(ro: vec3<f32>, rd: vec3<f32>, c: AurCurtain, t: f32, steps: i32) -> vec3<f32> {
    if rd.y < 0.002 {
        return vec3<f32>(0.0);
    }
    let nrm = vec2<f32>(-c.axis.y, c.axis.x);
    let rel = ro.xz - c.centre;
    let au = dot(rel, c.axis);
    let av = dot(rel, nrm);
    let bu = dot(rd.xz, c.axis);
    var bv = dot(rd.xz, nrm);
    if abs(bv) < 1e-4 {
        bv = select(-1e-4, 1e-4, bv >= 0.0);
    }
    // The slab the folded sheet lives in, cut to its height and length.
    let reach = c.amp * 1.6 + c.w * 2.5;
    let sa = (-reach - av) / bv;
    let sb = (reach - av) / bv;
    var lo = max(min(sa, sb), 0.0);
    var hi = min(max(sa, sb), AUR_FAR);
    lo = max(lo, (c.base - 1.0 - c.w - ro.y) / rd.y);
    hi = min(hi, (c.base + c.height * (3.2 + 2.5 * params.output.w) - ro.y) / rd.y);
    if abs(bu) > 1e-4 {
        let ua = (-c.half_len - au) / bu;
        let ub = (c.half_len - au) / bu;
        lo = max(lo, min(ua, ub));
        hi = min(hi, max(ua, ub));
    } else if abs(au) > c.half_len {
        return vec3<f32>(0.0);
    }
    if hi <= lo {
        return vec3<f32>(0.0);
    }

    let vis = params.output.w;
    // Samples sit on a grid along the ray (close enough for the linear fit
    // to hold against the sheet's thickness, wider when the slab is long), plus
    // the slab's two ends. A grid point entering the slab only splits an
    // end segment, so neighbouring pixels never jump.
    let grid = max((0.35 + c.w) / clamp(params.tune.z, 0.5, 2.0), (hi - lo) / f32(steps));
    let k0 = floor(lo / grid) + 1.0;
    let count = i32(max(ceil(hi / grid) - k0, 0.0));
    var s0 = lo;
    var g0 = av + bv * s0 - aur_fold(au + bu * s0, c, t);
    var e0 = aur_erf(g0 / c.w);
    var x0 = exp(-g0 * g0 / (c.w * c.w));
    var col_sum = vec3<f32>(0.0);
    var e_sum = 0.0;
    // A little absorption, front to back: a fold seen edge-on glows but
    // its far part, lower in the sky, no longer shows through as a block.
    var trans = 1.0;
    for (var k = 0; k <= count; k++) {
        let s1 = select(hi, min((k0 + f32(k)) * grid, hi), k < count);
        let ds = s1 - s0;
        let g1 = av + bv * s1 - aur_fold(au + bu * s1, c, t);
        let e1 = aur_erf(g1 / c.w);
        let x1 = exp(-g1 * g1 / (c.w * c.w));
        let d = g1 - g0;
        // The Gaussian sheet integrated along the segment, with the
        // sideways offset taken as linear between the samples, and where
        // on the segment that light comes from (its weighted centre), so
        // shading moves smoothly however coarse the segment.
        var amount = ds * x1;
        var tau = 0.5;
        if abs(d) > 1e-3 {
            let j = c.w * 0.8862 * (e1 - e0);
            amount = ds * j / d;
            if abs(j) > 1e-6 {
                tau = clamp((-0.5 * c.w * c.w * (x1 - x0) - g0 * j) / (d * j), 0.0, 1.0);
            }
        }
        // Only segments that touch the sheet (not its far tails) shade.
        if g0 * g1 < 0.0 || min(abs(g0), abs(g1)) < 1.8 * c.w {
            let s = s0 + tau * ds;
            let u = au + bu * s;
            let h = ro.y + rd.y * s;
            let u01 = clamp(u / (2.0 * c.half_len) + 0.5, 0.0, 1.0);

            // Rays: broad bands and fine striations drifting sideways
            // over seconds; the highs make the finest ones shimmer.
            let broad = aur_vn(u * 0.8 + t * 0.03, c.salt);
            let fine = aur_vn(u * 7.0 - t * 0.16, c.salt + 1);
            let ridge = 1.0 - abs(fine * 2.0 - 1.0);
            let far_fade = 1.0 - smoothstep(12.0, 45.0, s);
            let shimmer = 0.5 * sin(u * 23.0 + t * 0.7 + fine * 9.0);
            let rays = 0.35 + 0.55 * broad + 0.25 * ridge * ridge * far_fade
                + shimmer * (0.05 + 0.2 * params.env.z) * far_fade;

            // Height: a soft lower edge that undulates, a long fade upward,
            // taller where the rays are strong. In the visualiser the bands
            // set the height along the curtain, low to high.
            let base = c.base + 0.28 * sin(0.33 * u + c.phase * 1.3 + t * 0.05);
            let dh = h - base;
            let band_h = mix(1.0, 0.35 + 1.5 * band_at(mix(0.04, 0.96, u01)), vis);
            let breathe = 0.85 + 0.25 * params.env.w;
            let hl = c.height * (0.55 + 0.75 * broad) * band_h * breathe;
            let up = max(dh, 0.0);
            let prof = smoothstep(-0.35 - c.w * 0.6, 0.45 + c.w * 0.6, dh) * exp(-up / hl)
                * (1.0 + 0.9 * exp(-up / 0.4));
            // Soft ends along its length.
            let ends = 1.0 - smoothstep(0.62, 1.0, abs(u01 * 2.0 - 1.0));

            let w = amount * prof * max(rays, 0.0) * ends * trans;
            trans *= exp(-amount * prof * ends * 0.9);
            let tint = mix(c.low, c.high, smoothstep(0.0, 1.4, up / hl));
            col_sum += tint * w;
            e_sum += w;
        }
        s0 = s1;
        g0 = g1;
        e0 = e1;
        x0 = x1;
    }
    // Edge-on folds gather more light; ease them so they glow, not burn.
    let soft = (1.0 - exp(-e_sum * 1.8)) / max(e_sum * 1.8, 1e-4);
    let level = 0.75 + 0.35 * params.env.w + 0.15 * params.env.x;
    return col_sum * soft * c.gain * level;
}

// The night: a deep gradient with a faint glow along the horizon.
fn aur_night(rd: vec3<f32>, glow: vec3<f32>) -> vec3<f32> {
    // Night blue with a trace of the cover; kept near grey so the light
    // look (which stretches hue) stays calm.
    let tint = mix(vec3<f32>(dot(pal(3), LUMA)), pal(3), 0.2) * 0.6 + vec3<f32>(0.03, 0.035, 0.045);
    let up = clamp(rd.y, 0.0, 1.0);
    var col = tint * mix(0.12, 0.035, pow(up, 0.5));
    col += glow * 0.05 * exp(-abs(rd.y) * 9.0);
    return col;
}

fn aur_stars(rd: vec3<f32>, t: f32) -> f32 {
    let az = atan2(rd.x, rd.z);
    let el = asin(clamp(rd.y, -1.0, 1.0));
    let cell = 0.022;
    let q = vec2<f32>(az * cos(el), el) / cell;
    let id = floor(q);
    let h = hash21(id + vec2<f32>(79.19, 1047.29));
    if h < 0.965 {
        return 0.0;
    }
    let off = vec2<f32>(hash21(id + vec2<f32>(13.0, 71.0)), hash21(id + vec2<f32>(59.0, 3.0))) * 0.6 + 0.2;
    let d = (fract(q) - off) * cell * AUR_FOCAL;
    // A soft point a couple of logical pixels wide.
    let r2 = dot(d, d) / (0.0042 * 0.0042);
    let bright = pow((h - 0.965) / 0.035, 2.0);
    let twinkle = 0.8 + 0.2 * sin(t * (0.4 + h * 0.5) + h * 40.0);
    return exp(-r2) * bright * twinkle * smoothstep(0.03, 0.2, rd.y);
}

// Low hills along the horizon, as an elevation for each bearing.
fn aur_hills(rd: vec3<f32>) -> f32 {
    let az = atan2(rd.x, rd.z);
    let s = params.seed.z * 37.0;
    let n = 0.6 * aur_vn(az * 4.0 + s, 50) + 0.4 * aur_vn(az * 11.0 + s, 51);
    return (0.004 + 0.035 * n * n) * mix(0.5, 1.2, fract(params.seed.z * 5.3));
}

// Everything above the ground seen along a ray from `ro`: night, stars,
// hills and the curtains (`all`: the third, farthest one too). `quiet`
// dims the curtains (the left third).
fn aur_sky(ro: vec3<f32>, rd: vec3<f32>, c0: AurCurtain, c1: AurCurtain, c2: AurCurtain,
    t: f32, steps: i32, all: bool, quiet: f32, glow: vec3<f32>) -> vec3<f32> {
    var col = aur_night(rd, glow);
    let ridge = aur_hills(rd);
    if rd.y < ridge {
        // The hills: dark, a little of the aurora's glow on them.
        return aur_night(vec3<f32>(rd.x, 0.0, rd.z), glow) * 0.45 + glow * 0.006;
    }
    var aur = aur_curtain(ro, rd, c0, t, steps);
    aur += aur_curtain(ro, rd, c1, t, max(steps * 3 / 4, 6));
    if all {
        aur += aur_curtain(ro, rd, c2, t, max(steps / 2, 6));
    }
    aur *= quiet;
    let light = select(1.0, 0.25, params.output.z > 0.5);
    let stars = aur_stars(rd, t) * 0.5 * light / (1.0 + dot(aur, LUMA) * 6.0);
    // The air near the horizon softens the curtains' lower ends.
    let air = smoothstep(ridge, ridge + 0.12, rd.y) * 0.75 + 0.25;
    col += aur * air + vec3<f32>(stars) * mix(vec3<f32>(1.0), glow, 0.2);
    return col;
}

// Light the curtains throw on the ground at `p`.
fn aur_ground_light(p: vec2<f32>, c: AurCurtain, t: f32) -> vec3<f32> {
    let nrm = vec2<f32>(-c.axis.y, c.axis.x);
    let rel = p - c.centre;
    let u = dot(rel, c.axis);
    let v = dot(rel, nrm) - aur_fold(u, c, t);
    let ends = 1.0 - smoothstep(c.half_len * 0.6, c.half_len, abs(u));
    return mix(c.low, c.high, 0.25) * c.gain * ends * exp(-abs(v) * 0.3);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let frag = in.position.xy;
    let backdrop = params.output.w < 0.5;
    let t = params.clock.x;
    let sp = screen(frag);

    // A still camera on the shore, turning only very slowly.
    let yaw = 0.035 * sin(t * 0.023 + params.seed.w * 6.2831);
    let pitch = 0.27;
    let fw = vec3<f32>(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch));
    let rt = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fw));
    let up = cross(fw, rt);
    let rd = normalize(fw * AUR_FOCAL + rt * sp.x + up * sp.y);
    let ro = vec3<f32>(0.0, AUR_CAM_H, 0.0);

    let c0 = aur_make(0);
    let c1 = aur_make(1);
    let c2 = aur_make(2);
    let glow = (c0.low + c1.low * 0.6) * (0.5 + 0.3 * params.env.w);
    let steps = clamp(i32(24.0 * params.tune.z), 8, 48);
    // The backdrop keeps its left third quiet for the cover and the text.
    let quiet = select(1.0, smoothstep(-1.0, 0.15, sp.x), backdrop);
    let lake = params.seed.z > 0.42;

    var col: vec3<f32>;
    if rd.y >= 0.0 {
        col = aur_sky(ro, rd, c0, c1, c2, t, steps, true, quiet, glow);
    } else {
        let tg = -ro.y / rd.y;
        let p = ro + rd * tg;
        let horizon = aur_night(vec3<f32>(rd.x, 0.0, rd.z), glow);
        if lake {
            // A calm lake: slow, long swells; the highs stir it faintly.
            let wp = p.xz * vec2<f32>(0.9, 2.2) + vec2<f32>(t * 0.05, t * 0.11);
            let calm = 1.0 - smoothstep(3.0, 30.0, tg);
            let ripple = (noised(wp).yz + 0.5 * noised(wp * 2.7 + 3.1).yz)
                * (0.006 + 0.01 * params.env.z) * calm;
            // Mostly towards the eye: a mirror smeared a little vertically.
            let n = normalize(vec3<f32>(ripple.x * 0.3, 1.0, ripple.y));
            var r = reflect(rd, n);
            r.y = max(r.y, 0.001);
            let fres = 0.04 + 0.96 * pow(1.0 - max(dot(-rd, n), 0.0), 5.0);
            // A modest GPU (Adreno, GL) mirrors the two nearer curtains.
            let mirror = aur_sky(vec3<f32>(p.x, 0.0, p.z), r, c0, c1, c2, t,
                max(steps * 2 / 3, 6), params.clock.w < 0.5, quiet, glow);
            let deep = pal(3) * 0.01;
            col = mix(deep, mirror, mix(0.35, 0.95, fres));
        } else {
            // A snow plain with soft drifts, lit by the curtains above it.
            let dune = noised(p.xz * 0.35 + vec2<f32>(params.seed.z * 50.0, 0.0));
            let dn = normalize(vec3<f32>(dune.y * 0.25, 1.0, dune.z * 0.25));
            var lit = aur_ground_light(p.xz, c0, t) + aur_ground_light(p.xz, c1, t) * 0.7;
            lit *= quiet * (0.75 + 0.35 * params.env.w);
            let toward = normalize(vec3<f32>(c0.centre.x - p.x, 3.0, c0.centre.y - p.z));
            let shade = 0.55 + 0.45 * max(dot(dn, toward), 0.0);
            let snow = vec3<f32>(0.85, 0.88, 0.95);
            col = snow * (aur_night(vec3<f32>(0.0, 1.0, 0.0), glow) * 1.4 + lit * 0.16 * shade);
        }
        // Distance haze into the horizon.
        col = mix(col, horizon * 0.8, 1.0 - exp(-tg * 0.05));
    }

    // Strength: how far the scene stands out from the plain night.
    let flat_col = aur_night(vec3<f32>(rd.x, abs(rd.y), rd.z), glow);
    col = mix(flat_col, col, clamp(params.tune.x, 0.0, 1.5));
    // The light look stretches whatever hue it is given to its gamut, so
    // give it a grey sky to press: the curtains then read as pastels.
    if squeezed() {
        col = col * 0.8 + vec3<f32>(0.22);
    }
    let v = in.uv - 0.5;
    col *= 1.0 - 0.45 * dot(v, v) * select(1.0, 1.6, backdrop);
    return finish(col, frag);
}
