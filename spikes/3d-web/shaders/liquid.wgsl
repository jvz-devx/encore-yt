// Concept 3, liquid form: a slowly morphing glossy shape lit in the
// cover's colours. The bass swells it, the highs ripple its surface, both
// through slow envelopes; nothing jumps on a kick.

fn spin(p: vec3<f32>, t: f32) -> vec3<f32> {
    let xz = rotate2(t * 0.11) * p.xz;
    let q = vec3<f32>(xz.x, p.y, xz.y);
    let yz = rotate2(sin(t * 0.07) * 0.5) * q.yz;
    return vec3<f32>(q.x, yz.x, yz.y);
}

// Signed distance to the form. Its own seed makes each song's shape a
// little different without changing its character.
fn form(p: vec3<f32>) -> f32 {
    let t = params.clock.x;
    let s = params.seed * 6.2831;
    let q = spin(p, t);
    let swell = 1.0 + 0.1 * params.env.x + 0.03 * params.env.w;
    var d = length(q) - 1.05 * swell;
    // Two slow layers of morph.
    d += 0.2 * sin(q.x * 1.7 + t * 0.31 + s.x) * sin(q.y * 1.9 - t * 0.23 + s.y) * sin(q.z * 1.5 + t * 0.19 + s.z);
    d += 0.07 * sin(q.x * 3.3 - t * 0.41 + s.w) * sin(q.y * 2.9 + t * 0.37) * sin(q.z * 3.7 - t * 0.29 + s.x);
    // The highs: a fine ripple that fades in and out with their envelope.
    let ripple = 0.018 * params.env.z;
    d += ripple * sin(q.x * 11.0 + t * 1.3) * sin(q.y * 12.0 - t * 1.1) * sin(q.z * 10.0 + t * 0.9);
    // Mid-band shimmer along the form, low to high round its equator.
    let around = atan2(q.z, q.x) / (2.0 * PI) + 0.5;
    d -= 0.03 * band_at(around) * smoothstep(0.6, 0.0, abs(q.y)) * params.output.w;
    return d * 0.7;
}

fn form_normal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.0015, -0.0015);
    return normalize(e.xyy * form(p + e.xyy) + e.yyx * form(p + e.yyx)
        + e.yxy * form(p + e.yxy) + e.xxx * form(p + e.xxx));
}

// The studio the form reflects: a gradient of the palette and two soft
// light boxes in its colours, slowly turning.
fn studio(rd: vec3<f32>) -> vec3<f32> {
    let t = params.clock.x * 0.05;
    let up = rd.y * 0.5 + 0.5;
    var col = mix(pal(3) * 0.12, mix(pal(0), pal(1), 0.5) * 0.35, smoothstep(0.1, 0.9, up));
    let key_dir = normalize(vec3<f32>(cos(t) * 0.8, 0.6, sin(t) * 0.8 - 0.4));
    let fill_dir = normalize(vec3<f32>(-cos(t * 0.7), -0.1, 0.6));
    let key = pow(max(dot(rd, key_dir), 0.0), 18.0);
    let fill = pow(max(dot(rd, fill_dir), 0.0), 6.0);
    col += mix(pal(1), vec3<f32>(1.0), 0.35) * key * (1.5 + 0.4 * params.env.x);
    col += pal(2) * fill * 0.6;
    return col;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let frag = in.position.xy;
    let backdrop = params.output.w < 0.5;
    let sp = screen(frag);
    // The backdrop sets the form right of centre and larger, behind the
    // cover and text on the left; the visualiser centres it.
    let shift = select(vec2<f32>(0.0), vec2<f32>(-0.55, 0.05), backdrop);
    let ro = vec3<f32>(0.0, 0.0, select(3.6, 3.0, backdrop));
    let rd = normalize(vec3<f32>(sp + shift, -1.7));

    // Background: the studio seen soft, plus a glow of the form's colour.
    let bg_dir = normalize(vec3<f32>(sp * 0.6, -1.0));
    var col = studio(bg_dir) * 0.45;
    let glow = exp(-dot(sp + shift, sp + shift) * 1.4);
    col += mix(pal(0), pal(2), 0.5) * glow * (0.18 + 0.08 * params.env.w);

    // Only rays through the form's bounding sphere march; most of the frame
    // is background.
    let bound = 1.75;
    let b = dot(ro, rd);
    let disc = b * b - (dot(ro, ro) - bound * bound);
    var hit = false;
    var t = 0.0;
    var t_out = -1.0;
    if disc > 0.0 {
        t = max(-b - sqrt(disc), 0.0);
        t_out = -b + sqrt(disc);
    }
    let steps = i32(f32(48) * params.tune.z);
    for (var i = 0; i < steps; i++) {
        if t >= t_out {
            break;
        }
        let d = form(ro + rd * t);
        if d < 0.0008 * t {
            hit = true;
            break;
        }
        t += d;
    }

    if hit {
        let p = ro + rd * t;
        let n = form_normal(p);
        let v = -rd;
        let ndv = max(dot(n, v), 0.0);
        let fres = 0.04 + 0.96 * pow(1.0 - ndv, 5.0);
        let r = reflect(rd, n);
        // Albedo drifts across the palette over the surface.
        let q = spin(p, params.clock.x);
        let k = 0.5 + 0.5 * sin(q.y * 1.3 + q.x * 0.7 + params.clock.x * 0.05);
        let albedo = mix(pal(0), pal(2), k) * 0.5;
        let key_dir = normalize(vec3<f32>(0.5, 0.7, 0.5));
        let dif = max(dot(n, key_dir), 0.0) * 0.6 + 0.25;
        // A light wrap round the edge, like light through a thick liquid.
        let rim = pow(1.0 - ndv, 3.0) * mix(pal(1), pal(2), 0.5) * 0.5;
        col = albedo * dif + studio(r) * fres * 1.3 + rim;
    }

    col = mix(studio(bg_dir) * 0.45, col, clamp(params.tune.x, 0.0, 1.5));
    let vg = in.uv - 0.5;
    col *= 1.0 - 0.5 * dot(vg, vg) * select(1.0, 1.6, backdrop);
    return finish(col, frag);
}
