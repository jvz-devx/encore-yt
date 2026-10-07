// New concept, nebula: a slow drift through a soft cloud of glowing gas in
// the cover's colours, with stars at several depths. The video id seeds
// the cloud (where it is dense, how its colours are arranged, the drift's
// direction), so each song has its own nebula and always the same one.
// The bass makes the cloud's inner glow breathe, the highs let the faint
// stars twinkle a little more, both through slow envelopes. In the
// visualiser a ring of the cloud swells with the bands.
//
// Cost: one bounded march (an ellipsoid, steps by the chord's length between
// its entry and exit, two octaves of a cheap 3D value noise per step,
// early out once the gas is opaque); the stars are hashed grids in screen
// space, not part of the march.

struct Neb {
    // The cloud's ellipsoid in camera space (camera at the origin, -z ahead).
    centre: vec3<f32>,
    radii: vec3<f32>,
    // The camera's position in the noise's world (the drift).
    cam: vec3<f32>,
    // The drift as seen on screen, for the stars' parallax.
    slide: vec2<f32>,
    // The palette's arrangement: core, body, edge, lanes.
    core: vec3<f32>,
    body: vec3<f32>,
    edge: vec3<f32>,
    lane: vec3<f32>,
};

// Value noise with a float hash (Dave Hoskins' hash11, four lattice
// corners per call): common's noise3 costs 24 integer PCG rounds, too many
// for every step of the march.
// A fixed rotation (about 40 degrees round (1, 1, 1)).
const NEB_TURN: mat3x3<f32> = mat3x3<f32>(
    0.843, 0.452, -0.295,
    -0.295, 0.843, 0.452,
    0.452, -0.295, 0.843,
);

fn neb_h4(n: vec4<f32>) -> vec4<f32> {
    var p = fract(n * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return fract(p);
}

fn neb_noise(x: vec3<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    let n = i.x + i.y * 57.0 + i.z * 113.0;
    let lo = neb_h4(vec4<f32>(n, n + 1.0, n + 57.0, n + 58.0));
    let hi = neb_h4(vec4<f32>(n + 113.0, n + 114.0, n + 170.0, n + 171.0));
    let z = mix(lo, hi, u.z);
    let y = mix(z.xy, z.zw, u.y);
    return mix(y.x, y.y, u.x);
}

fn neb_setup(backdrop: bool) -> Neb {
    let s = params.seed;
    let t = params.clock.x;
    var n: Neb;
    // Backdrop: right of centre and a little high, so the cover and the
    // title on the left stay over quiet sky. Visualiser: centred.
    let wander = vec3<f32>(fract(s.x * 7.13) - 0.5, fract(s.y * 5.71) - 0.5, 0.0);
    n.centre = select(vec3<f32>(0.0, 0.0, -5.2) + wander * vec3<f32>(0.25, 0.2, 0.0),
        vec3<f32>(1.75, 0.25, -5.4) + wander * vec3<f32>(0.5, 0.5, 0.0), backdrop);
    n.radii = vec3<f32>(mix(2.6, 3.3, s.z), mix(1.9, 2.5, fract(s.z * 3.7)), 2.8);
    // The drift: mostly ahead, sideways along the seeded heading.
    let heading = s.w * 2.0 * PI;
    let side = vec2<f32>(cos(heading), sin(heading) * 0.6);
    let speed = 0.07;
    let offset = vec3<f32>(s.x * 97.0, s.y * 89.0, s.z * 83.0);
    n.cam = offset + vec3<f32>(side * 0.6, -1.0) * (t * speed);
    n.slide = side * (t * speed * 0.6);
    // Colours: a seeded rotation (and mirror) of the four cover colours.
    let k = i32(floor(s.y * 3.999));
    let dir = select(1, 3, s.z > 0.5);
    let c0 = pal(k % 4);
    let c1 = pal((k + dir) % 4);
    let c2 = pal((k + 2 * dir) % 4);
    let c3 = pal((k + 3 * dir) % 4);
    n.core = mix(c0, vec3<f32>(dot(c0, LUMA)), 0.15) * 1.2 + vec3<f32>(0.05);
    n.body = c1;
    n.edge = c2 * 0.8;
    n.lane = mix(c3, c0, 0.35) * 1.1 + vec3<f32>(0.03);
    return n;
}

// The gas at camera-space `p`: x density, y the colour mix (body to
// edge), z the lanes' glow, w the core's closeness 0..1. `shell_band` is
// the visualiser's band level for this pixel (0 in the backdrop).
fn neb_sample(p: vec3<f32>, n: Neb, viz: f32, shell_band: f32) -> vec4<f32> {
    let q = (p - n.centre) / n.radii;
    let r = length(q);
    let shape = smoothstep(0.94, 0.15, r);
    // Visualiser: a ring of gas round the view axis (a fat torus) whose
    // radius and density follow the bands round it; the middle thins out.
    var shell = 0.0;
    var hollow = 1.0;
    if viz > 0.5 {
        let rc = length(q.xy);
        let rs = 0.45 + 0.3 * shell_band;
        let x = (rc - rs) / (0.1 + 0.07 * shell_band);
        let z = q.z / 0.45;
        shell = exp(-x * x - z * z) * (0.7 + 1.8 * shell_band);
        hollow = (0.12 + 0.88 * smoothstep(0.1, rs, rc)) * smoothstep(rs + 0.45, rs, rc);
    }
    if shape <= 0.0 && shell <= 0.002 {
        return vec4<f32>(0.0);
    }
    let t = params.clock.x;
    let w = p + n.cam;
    // Off the lattice's axes, so value noise's square edges don't show.
    let wr = NEB_TURN * w;
    let n1 = neb_noise(wr * 0.5);
    // The threshold falls towards the core, so the middle is denser.
    let th = mix(0.64, 0.3, shape);
    let ridge = 1.0 - abs(n1 * 2.0 - 1.0);
    // Early out: the coarse octave alone says there's neither gas (the
    // finer terms add at most 0.44) nor a lane here.
    if n1 * 0.6 + 0.44 < th && ridge < 0.6 && shell < 0.01 {
        return vec4<f32>(0.0);
    }
    // The fine octave drifts slowly against the coarse one, so the gas
    // evolves even while the camera's drift is slow.
    let churn = vec3<f32>(t * 0.019, -t * 0.013, t * 0.011);
    let n2 = neb_noise(wr.yzx * 1.25 + vec3<f32>(n1 * 1.7) + churn);
    // Fine wisps without a third noise octave: sines bent by the noise.
    let wisp = sin(w.x * 2.9 + n2 * 6.0) * sin(w.y * 2.6 - n1 * 5.0 + w.z * 1.3);
    let f = n1 * 0.6 + n2 * 0.34 + 0.1 * wisp;
    // Clumps and gaps.
    var dens = smoothstep(th, th + 0.22, f) * sqrt(shape);
    // Visualiser: the gas outside the ring thins out.
    dens *= mix(1.0, hollow * 0.6, viz);
    dens += shell * smoothstep(0.2, 0.65, f);
    // Lanes: thin sheets where the coarse noise crosses its middle, only
    // where the fine one allows, so there are a few, not a web.
    let lane = pow(ridge, 7.0) * smoothstep(0.5, 0.75, n2) * smoothstep(0.95, 0.35, r);
    let mixc = clamp(0.5 + q.x * 0.4 - q.y * 0.3 + (n2 - 0.5) * 1.2, 0.0, 1.0);
    // The core's glow (brighter where the gas is denser) and the ring's.
    let core = exp(-r * r * 3.0) * (0.6 + 0.8 * f) + shell * 0.35;
    return vec4<f32>(dens, mixc, lane, core);
}

// Stars: three hashed grids, far ones finer, fainter and slower.
fn neb_stars(sp: vec2<f32>, n: Neb, px: f32) -> vec3<f32> {
    var col = vec3<f32>(0.0);
    let t = params.clock.x;
    let highs = params.env.z;
    for (var k = 0; k < 3; k++) {
        let fk = f32(k);
        // Layer 0 far, 2 near.
        let cells = mix(34.0, 13.0, fk * 0.5);
        let parallax = mix(0.1, 0.55, fk * 0.5);
        let uv = (sp + n.slide * parallax) * cells + vec2<f32>(fk * 37.1, fk * 11.7);
        let cell = vec2<i32>(floor(uv));
        let h = hash2i(cell + vec2<i32>(k * 101, k * 57));
        let keep = mix(0.26, 0.16, fk * 0.5);
        if h > keep {
            continue;
        }
        let h2 = fract(h * 97.31 + 0.13);
        let h3 = fract(h * 571.7 + 0.71);
        let at = vec2<f32>(0.2 + 0.6 * h2, 0.2 + 0.6 * h3);
        // Distance in pixels (a cell is px / 2 / cells pixels wide).
        let d = length(fract(uv) - at) * px * 0.5 / cells;
        let bright = pow(h / keep, 4.0);
        let faint = 1.0 - bright;
        // Twinkle over seconds: the faint ones, more with the highs.
        let rate = 0.3 + 0.5 * h2;
        let amount = faint * (0.1 + 0.4 * highs);
        let tw = 1.0 + amount * sin(t * rate + h3 * 6.2831);
        let radius = (0.6 + 0.5 * bright + 0.2 * fk) * px / 800.0;
        var s = exp(-d * d / (radius * radius));
        // A faint halo round the near, bright ones only.
        s += 0.05 * bright * fk * exp(-d / (radius * 4.0));
        // Exactly nothing by the cell's edge: the dark look lifts even a
        // trace above the sky, and a clipped halo shows as a square.
        s *= smoothstep(0.19, 0.1, length(fract(uv) - at));
        let lum = mix(0.05, 0.8, bright) * mix(0.4, 1.0, fk * 0.5) * tw;
        // Tinted a little by the cloud's colours, never white.
        let tint = mix(vec3<f32>(0.85, 0.87, 0.9), mix(n.core, n.lane, h2), 0.3);
        col += tint * s * lum;
    }
    return col;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let frag = in.position.xy;
    let backdrop = params.output.w < 0.5;
    let viz = params.output.w;
    let light = params.output.z > 0.5;
    let n = neb_setup(backdrop);
    let sp = screen(frag);
    let px = min(params.output.x, params.output.y);
    let t = params.clock.x;

    // A slow look round, a few degrees over tens of seconds.
    let look = vec2<f32>(sin(t * 0.017 + params.seed.x * 6.0), sin(t * 0.013 + params.seed.y * 6.0)) * 0.035;
    let rd = normalize(vec3<f32>(sp + look, -1.7));

    // The sky: near black with a trace of the edge colour, a soft haze of
    // the body's colour round the cloud.
    let focus = n.centre.xy / -n.centre.z * 1.7 - look;
    let fd = sp - focus;
    let haze = exp(-dot(fd, fd) * 0.8);
    let night = mix(n.edge, vec3<f32>(dot(n.edge, LUMA)), 0.6) * 0.01;
    let sky = night + n.body * haze * (0.04 + 0.02 * params.env.x);

    // The visualiser's band for this pixel: round the cloud's centre on
    // screen, mirrored so there's no seam, three taps so it's soft.
    var shell_band = 0.0;
    if viz > 0.5 {
        let around = abs(fract(atan2(fd.y, fd.x) / (2.0 * PI) + 0.25) * 2.0 - 1.0);
        shell_band = (band_at(around - 0.04) + band_at(around) + band_at(around + 0.04)) / 3.0;
    }

    // The march, only between the ellipsoid's entry and exit.
    var col = vec3<f32>(0.0);
    var trans = 1.0;
    let bound = select(0.94, 1.1, viz > 0.5);
    let o = -n.centre / (n.radii * bound);
    let d = rd / (n.radii * bound);
    let a = dot(d, d);
    let b = dot(o, d);
    let c = dot(o, o) - 1.0;
    let disc = b * b - a * c;
    if disc > 0.0 {
        let sq = sqrt(disc);
        let t0 = max((-b - sq) / a, 0.0);
        let t1 = (-b + sq) / a;
        // Steps by the chord's length: 16 through the middle at quality 1,
        // fewer through the edges, where the path is short.
        let most = max(16.0 * params.tune.z, 6.0);
        let chord = t1 - t0;
        let steps = i32(clamp(ceil(most * chord / (2.0 * n.radii.z * bound)), 3.0, most));
        let dt = chord / f32(steps);
        // Jitter the start per pixel so the steps don't band; interleaved
        // gradient noise spreads it finer than a hash, so it reads as
        // softness rather than grain.
        let ign = fract(52.982918 * fract(dot(frag, vec2<f32>(0.06711056, 0.00583715))));
        var tt = t0 + dt * ign;
        let glow = 0.7 + 0.8 * params.env.x + 0.15 * params.env.w;
        // The visualiser's ring is thinner gas: dimmer and more transparent.
        let absorb = select(0.7, 0.4, viz > 0.5);
        let emit = select(1.0, 0.55, viz > 0.5);
        for (var i = 0; i < steps; i++) {
            let g = neb_sample(rd * tt, n, viz, shell_band);
            if g.x > 0.0 || g.z > 0.0 {
                let alpha = 1.0 - exp(-g.x * dt * absorb);
                // Emission: body to edge across the cloud, the core colour
                // rising towards the middle and breathing with the bass.
                var e = mix(n.body, n.edge, g.y) * 0.5;
                e = mix(e, n.core, min(g.w, 1.0) * 0.75) * (1.0 + g.w * glow * 1.4);
                col += trans * alpha * e * emit;
                // The lanes glow without absorbing.
                col += trans * g.z * dt * n.lane * 0.4;
                trans *= 1.0 - alpha;
                if trans < 0.04 {
                    break;
                }
            }
            tt += dt;
        }
    }

    // Stars behind the gas, the near layer partly in front of it. Fewer
    // and softer in the light look, where they'd read as specks.
    let stars = neb_stars(sp, n, px) * select(1.0, 0.4, light);
    var scene = sky * trans + col + stars * mix(trans, 1.0, 0.2);

    // The light look lifts luminance into a narrow band and stretches the
    // chroma of what's dark: keep the sky grey and give the cloud more
    // range, so it shows as a soft tint instead of a saturated wash.
    if light && backdrop {
        let lum = dot(scene, LUMA);
        scene = mix(vec3<f32>(lum), scene, 0.4 * smoothstep(0.0, 0.12, lum)) * 2.6;
    }

    // Backdrop: keep the left third quiet under the cover and title.
    if backdrop {
        scene *= mix(0.3, 1.0, smoothstep(-1.45, -0.1, sp.x));
    }

    // Strength: how far the cloud stands out from the sky.
    let flat_col = night + n.body * haze * 0.04;
    scene = mix(flat_col, scene, clamp(params.tune.x, 0.0, 1.5));
    let vg = in.uv - 0.5;
    scene *= 1.0 - 0.5 * dot(vg, vg) * select(1.0, 1.6, backdrop);
    return finish(scene, frag);
}
