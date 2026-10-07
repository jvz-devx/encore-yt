// Concept "Glass": a few thick, softly bevelled glass objects (a tall pane,
// a prism, a thick round puck) turning slowly in front of a soft luminous
// field of the cover's colours. One refraction bounce (in and out, the way
// out split per channel for a little dispersion at the bevels), a soft
// studio in the reflections, faint caustics on the field behind.
//
// The field is a plane behind the objects, so what the glass shows is the
// same field, bent. Shapes are signed distances marched from their
// bounding boxes, so a few steps reach them.
//
// Music: the field's blobs breathe with the bass envelope and brighten with
// the level; the objects turn on the scene clock, which runs a little
// faster while the music is loud. In the visualiser the bands tilt and
// space the objects, gently.
// Seed: the blobs' places and phases, which cover colour each blob takes,
// the objects' turning phases.

const GLASS_PLANE: f32 = 3.0;
const GLASS_CAM: f32 = 5.0;
const GLASS_FOCAL: f32 = 2.2;
const GLASS_IOR: f32 = 1.5;
const GLASS_DISPERSION: f32 = 0.025;
const GLASS_BLOBS: i32 = 5;

struct Glass {
    c: vec3<f32>,
    rot: mat3x3<f32>,
    // xy: the outline's half size before the bevel (the prism: x its
    // triangle's half side), z: the outline's corner radius, w: half thickness
    size: vec4<f32>,
    bevel: f32,
    // 0 pane, 1 prism, 2 puck
    kind: i32,
    frost: f32,
    bound: vec3<f32>,
    tint: vec3<f32>,
};

var<private> GP: array<vec3<f32>, 4>;
var<private> GO: array<Glass, 3>;
// Blobs: xy centre on the field, z radius, w amplitude; and their colours.
var<private> GB: array<vec4<f32>, 5>;
var<private> GBC: array<vec3<f32>, 5>;

fn glass_light() -> vec3<f32> {
    return normalize(vec3<f32>(0.22, -0.32, -1.0));
}

fn glass_rot(yaw: f32, pitch: f32, roll: f32) -> mat3x3<f32> {
    let cy = cos(yaw);
    let sy = sin(yaw);
    let cx = cos(pitch);
    let sx = sin(pitch);
    let cz = cos(roll);
    let sz = sin(roll);
    let ry = mat3x3<f32>(cy, 0.0, -sy, 0.0, 1.0, 0.0, sy, 0.0, cy);
    let rx = mat3x3<f32>(1.0, 0.0, 0.0, 0.0, cx, sx, 0.0, -sx, cx);
    let rz = mat3x3<f32>(cz, sz, 0.0, -sz, cz, 0.0, 0.0, 0.0, 1.0);
    return ry * rx * rz;
}

fn glass_rrect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Inigo Quilez's equilateral triangle, `r` its half side, centred on its centroid.
fn glass_tri(p_in: vec2<f32>, r: f32) -> f32 {
    let k = sqrt(3.0);
    var p = vec2<f32>(abs(p_in.x) - r, p_in.y + r / k);
    if p.x + k * p.y > 0.0 {
        p = vec2<f32>(p.x - k * p.y, -k * p.x - p.y) * 0.5;
    }
    p.x -= clamp(p.x, -2.0 * r, 0.0);
    return select(length(p), -length(p), p.y > 0.0);
}

fn glass_outline(o: Glass, p: vec2<f32>) -> f32 {
    if o.kind == 1 {
        return glass_tri(p, o.size.x);
    }
    return glass_rrect(p, o.size.xy, o.size.z);
}

// The object in its own space: the outline extruded, every edge bevelled.
fn glass_sd(o: Glass, p: vec3<f32>) -> f32 {
    let w = vec2<f32>(glass_outline(o, p.xy), abs(p.z) - (o.size.w - o.bevel));
    return min(max(w.x, w.y), 0.0) + length(max(w, vec2<f32>(0.0))) - o.bevel;
}

fn glass_normal(o: Glass, p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.0015, -0.0015);
    return normalize(e.xyy * glass_sd(o, p + e.xyy) + e.yyx * glass_sd(o, p + e.yyx)
        + e.yxy * glass_sd(o, p + e.yxy) + e.xxx * glass_sd(o, p + e.xxx));
}

// Ray against a centred box: x entry, y exit.
fn glass_box(ro: vec3<f32>, rd: vec3<f32>, b: vec3<f32>) -> vec2<f32> {
    let safe = select(rd, vec3<f32>(1e-6), abs(rd) < vec3<f32>(1e-6));
    let m = 1.0 / safe;
    let n = m * ro;
    let k = abs(m) * b;
    let t1 = -n - k;
    let t2 = -n + k;
    return vec2<f32>(max(max(t1.x, t1.y), t1.z), min(min(t2.x, t2.y), t2.z));
}

fn glass_bands(a: f32, b: f32) -> f32 {
    return (band_at(a) + band_at(mix(a, b, 0.33)) + band_at(mix(a, b, 0.66)) + band_at(b)) * 0.25;
}

fn glass_setup(t: f32, vis: bool) {
    let s = params.seed;
    let e = params.env;
    // The field's blobs, weighted to the right in the backdrop.
    var anchors = array<vec3<f32>, 5>(
        vec3<f32>(1.9, 0.9, 2.6),
        vec3<f32>(4.3, -1.1, 2.2),
        vec3<f32>(2.6, 2.7, 2.3),
        vec3<f32>(2.4, -2.8, 2.4),
        vec3<f32>(-3.8, 0.4, 2.0),
    );
    var amps = array<f32, 5>(0.5, 0.42, 0.4, 0.36, 0.1);
    let shift = select(0.0, -2.2, vis);
    let rot_pal = i32(s.z * 3.99);
    for (var k = 0; k < GLASS_BLOBS; k++) {
        let fk = f32(k);
        let ph = s.x * 6.2831 + fk * 1.7 + s.w * fk * 2.3;
        let a = anchors[k];
        let jitter = (vec2<f32>(fract(s.y * (3.1 + fk * 1.9)), fract(s.w * (2.3 + fk * 1.3))) - 0.5) * 1.4;
        let drift = vec2<f32>(sin(t * (0.05 + 0.013 * fk) + ph), cos(t * (0.041 + 0.011 * fk) + ph * 1.3)) * 0.9;
        var centre = a.xy + jitter + drift;
        centre.x += shift;
        // Breathing: the bass swells the even blobs, the mids the odd ones.
        let breathe = select(0.06 * e.y, 0.14 * e.x, k % 2 == 0);
        var amp = amps[k] * select(0.8 + 0.35 * e.w, 0.7 + 0.6 * e.w, vis);
        if vis && k == 4 {
            amp = 0.3;
        }
        GB[k] = vec4<f32>(centre, a.z * (1.0 + breathe), amp);
        let c = GP[(k + rot_pal) % 4];
        GBC[k] = mix(vec3<f32>(dot(c, LUMA)), c, 0.8);
    }
    GBC[4] = mix(GP[2], GP[3], 0.5);

    // The objects.
    let lows = select(0.0, glass_bands(0.0, 0.18), vis);
    let mids = select(0.0, glass_bands(0.25, 0.6), vis);
    let highs = select(0.0, glass_bands(0.62, 1.0), vis);
    let ph0 = s.x * 6.2831;
    let ph1 = s.y * 6.2831;
    let ph2 = s.w * 6.2831;

    var pane: Glass;
    pane.kind = 0;
    pane.c = select(vec3<f32>(0.75, 0.3, 0.0), vec3<f32>(-0.2, 0.3, 0.0), vis);
    pane.size = vec4<f32>(0.85, 1.15, 0.28, 0.16);
    pane.bevel = 0.1;
    pane.rot = glass_rot(-0.45 + 0.35 * sin(t * 0.043 + ph0) + 0.1 * e.w,
        0.12 * sin(t * 0.031 + ph0 * 1.3) + 0.14 * lows,
        0.08 + 0.05 * sin(t * 0.027 + ph1));
    pane.frost = 0.5;
    pane.tint = mix(vec3<f32>(1.0), GP[0], 0.12);

    var prism: Glass;
    prism.kind = 1;
    prism.c = select(vec3<f32>(2.05, -1.05, 0.5), vec3<f32>(1.45 + 0.35 * mids, -0.45, 0.5), vis);
    prism.size = vec4<f32>(0.36, 0.0, 0.0, 0.62);
    prism.bevel = 0.1;
    prism.rot = glass_rot(1.2 + 0.15 * sin(t * 0.023 + ph1),
        0.35 + 0.1 * sin(t * 0.019 + ph2) + 0.12 * mids,
        t * 0.045 + ph1);
    prism.frost = 0.0;
    prism.tint = mix(vec3<f32>(1.0), GP[1], 0.08);

    var puck: Glass;
    puck.kind = 2;
    puck.c = select(vec3<f32>(2.3, 1.1, 0.8), vec3<f32>(-1.75 - 0.3 * highs, -0.55, 1.0), vis);
    puck.size = vec4<f32>(0.32, 0.32, 0.32, 0.17);
    puck.bevel = 0.14;
    puck.rot = glass_rot(select(-0.2, 0.35, vis) + 0.3 * sin(t * 0.037 + ph2),
        -0.3 + 0.2 * sin(t * 0.029 + ph0) - 0.15 * highs,
        0.0);
    puck.frost = 0.25;
    puck.tint = mix(vec3<f32>(1.0), GP[2], 0.12);

    pane.bound = vec3<f32>(pane.size.xy + vec2<f32>(pane.bevel + 0.06), pane.size.w + 0.06);
    prism.bound = vec3<f32>(prism.size.x, prism.size.x * 2.0 / sqrt(3.0), prism.size.w)
        + vec3<f32>(prism.bevel + 0.06, prism.bevel + 0.06, 0.06);
    puck.bound = vec3<f32>(puck.size.xy + vec2<f32>(puck.bevel + 0.06), puck.size.w + 0.06);
    GO[0] = pane;
    GO[1] = prism;
    GO[2] = puck;
}

// The luminous field on the plane behind, `blur` widening its blobs (so
// frosted glass sees it softer, keeping each blob's energy).
fn glass_field(p: vec2<f32>, blur: f32) -> vec3<f32> {
    var col = mix(GP[3], GP[0], 0.3) * 0.035;
    col += mix(GP[1], GP[2], 0.5) * 0.03 * smoothstep(-4.0, 4.0, p.y);
    for (var k = 0; k < GLASS_BLOBS; k++) {
        let b = GB[k];
        let r2 = b.z * b.z + blur * blur;
        let d = p - b.xy;
        col += GBC[k] * (b.w * b.z * b.z / r2) * exp(-dot(d, d) / r2);
    }
    return col;
}

fn glass_project(x: vec3<f32>) -> vec2<f32> {
    let l = glass_light();
    return x.xy + l.xy * ((-GLASS_PLANE - x.z) / l.z);
}

fn glass_sq(x: f32) -> f32 {
    return x * x;
}

// Light the objects focus on the field: a bright rim inside each pane's
// shadow (the bevels gather it), a lens focus under the puck, and a
// dispersed streak beside the prism.
fn glass_caustic(o: Glass, p: vec2<f32>, blur: f32, light: vec3<f32>) -> vec3<f32> {
    let l = glass_light();
    let x = vec3<f32>(p, -GLASS_PLANE);
    var acc = vec3<f32>(0.0);
    // Far from where the object's light lands: nothing to add.
    let reach = length(o.bound) * 1.5 + 0.8 + blur;
    let centre = glass_project(o.c);
    if dot(p - centre, p - centre) < reach * reach {
        if o.kind == 1 {
            let a = o.rot[2] * o.size.w;
            let p0 = glass_project(o.c - a);
            let p1 = glass_project(o.c + a);
            let seg = p1 - p0;
            let len = max(length(seg), 1e-3);
            let dir = seg / len;
            let rel = p - p0;
            let u = dot(rel, dir) / len;
            let across = dir.x * rel.y - dir.y * rel.x;
            let w = 0.13 + blur * 0.3;
            let off = 0.5;
            let spec = vec3<f32>(exp(-glass_sq((across - off) / w)),
                exp(-glass_sq((across - off - 0.09) / w)),
                exp(-glass_sq((across - off - 0.18) / w)));
            let along = smoothstep(-0.05, 0.2, u) * smoothstep(1.05, 0.8, u);
            acc += mix(spec, vec3<f32>(spec.g) * light, 0.45) * along * 0.45;
        } else {
            let nrm = o.rot[2];
            let dn = dot(l, nrm);
            let s = dot(x - o.c, nrm) / select(dn, 1e-3, abs(dn) < 1e-3);
            let q = transpose(o.rot) * (x - l * s - o.c);
            let e = glass_outline(o, q.xy) - o.bevel;
            let w = 0.07 + 0.03 * abs(s) + blur * 0.3;
            let face = smoothstep(0.1, 0.5, abs(dn));
            let at = e + 0.6 * w;
            let rim = vec3<f32>(exp(-glass_sq((at + 0.025) / w)), exp(-glass_sq(at / w)),
                exp(-glass_sq((at - 0.025) / w)));
            let inner = smoothstep(w, -w, e) * 0.1;
            var lens = 0.0;
            if o.kind == 2 {
                lens = exp(-dot(q.xy, q.xy) / (0.02 + w * w)) * 0.5;
            }
            acc += (rim * 0.32 + vec3<f32>(inner + lens) * light) * face;
        }
    }
    return acc;
}

// The objects are taken one by one with constant indices, so their data
// stays in registers.
fn glass_caustics(p: vec2<f32>, blur: f32) -> vec3<f32> {
    let light = mix(vec3<f32>(1.0, 0.96, 0.9), GP[1], 0.35);
    let acc = glass_caustic(GO[0], p, blur, light) + glass_caustic(GO[1], p, blur, light)
        + glass_caustic(GO[2], p, blur, light);
    return acc * light * (0.14 + 0.06 * params.env.w);
}

// March to one object from its bounding box: x the distance, y how much
// of the pixel it covers (0 none). A miss that grazes the silhouette within
// a pixel or so counts with partial cover, for soft edges.
fn glass_march(o: Glass, ro: vec3<f32>, rd: vec3<f32>, steps: i32, pixel: f32) -> vec2<f32> {
    let rt = transpose(o.rot);
    let lro = rt * (ro - o.c);
    let lrd = rt * rd;
    let span = glass_box(lro, lrd, o.bound);
    if span.x >= span.y || span.y <= 0.0 {
        return vec2<f32>(1e9, 0.0);
    }
    var tt = max(span.x, 0.0);
    var dmin = 1e9;
    var tmin = tt;
    for (var k = 0; k < steps; k++) {
        let d = glass_sd(o, lro + lrd * tt);
        if d < dmin {
            dmin = d;
            tmin = tt;
        }
        if d < 0.0005 * tt {
            return vec2<f32>(tt, 1.0);
        }
        tt += d;
        if tt > span.y {
            break;
        }
    }
    return vec2<f32>(tmin, clamp(1.0 - dmin / (1.5 * pixel * tmin), 0.0, 1.0));
}

// The studio the glass reflects: a soft gradient, a softbox overhead and
// strips at the sides, turning a little over time.
fn glass_studio(d_in: vec3<f32>) -> vec3<f32> {
    let sway = rotate2(0.25 * sin(params.clock.x * 0.03)) * d_in.xz;
    let d = vec3<f32>(sway.x, d_in.y, sway.y);
    let up = d.y * 0.5 + 0.5;
    var col = mix(mix(GP[3], GP[0], 0.4) * 0.07, mix(GP[0], GP[1], 0.5) * 0.22, smoothstep(0.0, 1.0, up));
    let warm = mix(vec3<f32>(1.0, 0.96, 0.9), GP[1], 0.3);
    let top = smoothstep(0.45, 0.75, d.y) * smoothstep(0.8, 0.4, abs(d.x));
    let side = smoothstep(0.5, 0.8, d.x) * smoothstep(0.5, 0.2, abs(d.y - 0.1));
    let left = smoothstep(0.6, 0.9, -d.x) * smoothstep(0.6, 0.2, abs(d.y));
    col += warm * top * 1.1 + mix(warm, GP[2], 0.4) * side * 0.6 + GP[2] * left * 0.15;
    return col;
}

// What a ray leaving the glass sees: the field where it meets the plane,
// fading to the studio for rays that turn away from it.
fn glass_behind(o: vec3<f32>, d: vec3<f32>, blur: f32) -> vec3<f32> {
    let w = smoothstep(-0.02, -0.25, d.z);
    let s = (-GLASS_PLANE - o.z) / min(d.z, -1e-3);
    // Steep rays would read the dim far field; pull them in a little so a
    // prism's faces stay luminous.
    let off = d.xy * s;
    let f = glass_field(o.xy + off / (1.0 + 0.3 * length(off)), blur);
    return mix(glass_studio(d) * 0.8, f, w);
}

fn glass_exit(o: Glass, di: vec3<f32>, ne: vec3<f32>, eta: f32) -> vec3<f32> {
    let out = refract(di, -ne, eta);
    // Total internal reflection: one bounce only, take the mirrored ray.
    return select(out, reflect(di, -ne), dot(out, out) < 1e-6);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let frag = in.position.xy;
    let vis = params.output.w > 0.5;
    let t = params.clock.x;
    for (var i = 0; i < 4; i++) {
        GP[i] = pal(i);
    }
    glass_setup(t, vis);

    let sp = screen(frag);
    let ro = vec3<f32>(0.0, 0.0, GLASS_CAM);
    let rd = normalize(vec3<f32>(sp, -GLASS_FOCAL));
    let tb = (ro.z + GLASS_PLANE) / -rd.z;
    let pb = ro.xy + rd.xy * tb;
    let flat_col = glass_field(pb, 0.0);
    var col = flat_col + glass_caustics(pb, 0.0);

    // The nearest object along the ray.
    let steps = max(i32(24.0 * params.tune.z), 8);
    let pixel = 2.0 / (min(params.output.x, params.output.y) * GLASS_FOCAL);
    var best = 1e9;
    var hit_i = -1;
    var cover = 0.0;
    let m0 = glass_march(GO[0], ro, rd, steps, pixel);
    if m0.y > 0.0 {
        best = m0.x;
        hit_i = 0;
        cover = m0.y;
    }
    let m1 = glass_march(GO[1], ro, rd, steps, pixel);
    if m1.y > 0.0 && m1.x < best {
        best = m1.x;
        hit_i = 1;
        cover = m1.y;
    }
    let m2 = glass_march(GO[2], ro, rd, steps, pixel);
    if m2.y > 0.0 && m2.x < best {
        best = m2.x;
        hit_i = 2;
        cover = m2.y;
    }

    if hit_i >= 0 {
        var o = GO[0];
        if hit_i == 1 {
            o = GO[1];
        } else if hit_i == 2 {
            o = GO[2];
        }
        let rt = transpose(o.rot);
        let lro = rt * (ro - o.c);
        let lrd = rt * rd;
        // For a grazing miss, shade the surface point nearest the ray.
        let p0 = lro + lrd * best;
        let p = p0 - glass_normal(o, p0) * max(glass_sd(o, p0), 0.0);
        var n = glass_normal(o, p);
        // A grazing miss can sample a normal that faces away; turn it to
        // the silhouette so it reflects what the edge would.
        let back = dot(n, lrd);
        n = select(n, normalize(n - lrd * (back + 0.02)), back > -0.02);
        // Partly covered edge pixels are shaded as the silhouette itself
        // (their nearest surface point can face the camera and sparkle).
        let ndv = select(clamp(dot(n, -lrd), 0.0, 1.0), 0.0, cover < 0.999);
        // Grazing hits are where single pixels go wrong (the refracted
        // path turns wild); ease them back to the background seen directly.
        let graze = smoothstep(0.0, 0.15, ndv);
        let fres = (0.04 + 0.96 * pow(1.0 - ndv, 5.0)) * smoothstep(0.0, 0.12, ndv);
        let refl = glass_studio(o.rot * reflect(lrd, n));

        // Through the glass: in, across (marched back from the far side of
        // the box, the shapes are convex), and out per channel.
        var di = refract(lrd, n, 1.0 / GLASS_IOR);
        di = select(di, lrd, dot(di, di) < 1e-6);
        let span = glass_box(p, di, o.bound);
        let far = p + di * span.y;
        var s = 0.0;
        for (var k = 0; k < steps; k++) {
            let d = glass_sd(o, far - di * s);
            if d < 0.0005 {
                break;
            }
            s += d;
            if s > span.y {
                break;
            }
        }
        s = min(s, span.y);
        let pe = far - di * s;
        let inside = max(span.y - s, 0.0);
        let ne = glass_normal(o, pe);
        let wo = o.c + o.rot * pe;
        let dr = o.rot * glass_exit(o, di, ne, GLASS_IOR - GLASS_DISPERSION);
        let dg = o.rot * glass_exit(o, di, ne, GLASS_IOR);
        let db = o.rot * glass_exit(o, di, ne, GLASS_IOR + GLASS_DISPERSION);
        let blur = o.frost;
        var trans = vec3<f32>(glass_behind(wo, dr, blur).r, glass_behind(wo, dg, blur).g,
            glass_behind(wo, db, blur).b);
        // The caustics seen through it, from the middle ray only.
        if dg.z < -0.05 {
            let sg = (-GLASS_PLANE - wo.z) / dg.z;
            trans += glass_caustics(wo.xy + dg.xy * sg, blur);
        }
        // Frost: a milky lift, more for the frosted panes.
        let milk = mix(vec3<f32>(dot(trans, LUMA)), trans, 0.7);
        trans = mix(trans, milk, o.frost * 0.4) + o.tint * (0.012 + 0.03 * o.frost);
        let absorb = pow(o.tint, vec3<f32>(inside * 0.6));
        // A thin light along the rim, where the bevel turns away.
        let rim = pow(1.0 - ndv, 3.0) * smoothstep(0.0, 0.12, ndv) * mix(GP[1], vec3<f32>(1.0), 0.5) * 0.08;
        trans = mix(col, trans * absorb, graze);
        let body = trans * (1.0 - fres) + refl * fres + rim;
        col = mix(col, body, cover);
    }

    col = mix(flat_col, col, clamp(params.tune.x, 0.0, 1.5));
    let backdrop = !vis;
    let vg = in.uv - 0.5;
    col *= 1.0 - 0.45 * dot(vg, vg) * select(1.0, 1.6, backdrop);
    return finish(col, frag);
}
