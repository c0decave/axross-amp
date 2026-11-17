// The pixel faces of axross-amp: one module, a face chosen by a uniform.
// Colours come in as the scheme's (sRGB, 0..1); `u.face.y` says whether
// the target wants linear values. Everything is a function of the pixel,
// the time and the analysis - nothing is kept between frames here (the
// reaction-diffusion and the mel spectrogram come in as a texture).

struct U {
    rect: vec4<f32>,     // the face's area: x, y, width, height (pixels)
    clip: vec4<f32>,     // the rounded background: x, y, width, height
    t: vec4<f32>,        // time (s), beat pulse 0..1, energy, bass
    m: vec4<f32>,        // phase, middle, treble, corner radius (pixels)
    face: vec4<f32>,     // face, linear target (1), texture scroll, beats
    colours: array<vec4<f32>, 5>, // background, low, middle, high, peak
    bands: array<vec4<f32>, 8>,   // 32 bands, bass to treble, 0..1
    drops: array<vec4<f32>, 8>,   // ripples: x, y (0..1), age (s), strength
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var data: texture_2d<f32>;
@group(0) @binding(2) var data_sampler: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    // one triangle over the whole viewport
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var out: Out;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

const TAU: f32 = 6.2831853;

fn colour(i: i32) -> vec3<f32> {
    return u.colours[i].rgb;
}

// The scheme as one gradient: background, quiet, middle, loud, peak.
fn palette(t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    if x < 0.25 {
        return mix(colour(0), colour(1), x / 0.25);
    } else if x < 0.55 {
        return mix(colour(1), colour(2), (x - 0.25) / 0.3);
    } else if x < 0.8 {
        return mix(colour(2), colour(3), (x - 0.55) / 0.25);
    }
    return mix(colour(3), colour(4), (x - 0.8) / 0.2);
}

// Round the bar colours: quiet, middle, loud, quiet.
fn cycle(t: f32) -> vec3<f32> {
    let x = fract(t) * 3.0;
    if x < 1.0 {
        return mix(colour(1), colour(2), x);
    } else if x < 2.0 {
        return mix(colour(2), colour(3), x - 1.0);
    }
    return mix(colour(3), colour(1), x - 2.0);
}

fn band(x: f32) -> f32 {
    let f = clamp(x, 0.0, 1.0) * 31.0;
    let i = i32(floor(f));
    let j = min(i + 1, 31);
    let a = u.bands[i / 4][i % 4];
    let b = u.bands[j / 4][j % 4];
    return mix(a, b, f - f32(i));
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var q = p;
    for (var k = 0; k < 5; k = k + 1) {
        v = v + a * noise(q);
        q = q * 2.03 + vec2<f32>(1.7, 9.2);
        a = a * 0.5;
    }
    return v;
}

fn plasma(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x * (0.35 + 0.8 * u.t.z);
    let bass = u.t.w;
    var v = sin(p.x * 3.0 + t);
    v = v + sin((p.y * 3.0 + t) * 0.7);
    v = v + sin((p.x * 2.0 + p.y * 3.0 + t * 1.3) * (1.0 + bass));
    let c = p + 0.6 * vec2<f32>(sin(t * 0.33), cos(t * 0.5));
    v = v + sin(sqrt(dot(c, c) * 18.0 + 1.0) - t * 2.0);
    let shade = 0.55 + 0.45 * sin(v * 1.6 + u.m.x);
    return cycle(v * 0.18 + t * 0.05) * (0.35 + 0.65 * shade) + colour(4) * u.t.y * 0.15 * shade;
}

fn fire(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let t = u.t.x;
    let q = vec2<f32>(uv.x * aspect * 3.0, (1.0 - uv.y) * 3.0);
    let rise = vec2<f32>(0.0, t * (1.2 + 1.5 * u.t.z));
    let n = fbm(q - rise + vec2<f32>(fbm(q * 0.7 - rise * 0.5) * 1.3, 0.0));
    let height = 0.35 + 0.55 * band(uv.x * 0.6) + 0.3 * u.t.y;
    let y = 1.0 - uv.y;
    let heat = clamp(n * 1.4 - y / max(height, 0.05) + 0.25 + 0.25 * u.m.z * noise(q * 6.0 + t * 8.0), 0.0, 1.0);
    return palette(pow(heat, 0.8));
}

fn metaballs(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x * 0.6;
    var field = 0.0;
    var tint = vec3<f32>(0.0);
    for (var k = 0; k < 8; k = k + 1) {
        let fk = f32(k);
        let centre = vec2<f32>(
            0.7 * sin(t * (0.5 + 0.13 * fk) + fk * 1.7),
            0.4 * cos(t * (0.4 + 0.11 * fk) + fk * 2.3)
        );
        let level = band((fk + 0.5) / 8.0);
        let r = 0.06 + 0.16 * level + 0.05 * u.t.y;
        let d = p - centre;
        let f = r * r / max(dot(d, d), 1e-4);
        field = field + f;
        tint = tint + cycle(fk / 8.0 + t * 0.05) * f;
    }
    let base = tint / max(field, 1e-4);
    let edge = smoothstep(0.9, 1.1, field);
    let glow = clamp(field * 0.45, 0.0, 1.0);
    return mix(colour(0) + base * glow * 0.35, base + colour(4) * 0.25 * smoothstep(2.0, 5.0, field), edge);
}

fn aurora(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let t = u.t.x * 0.25;
    var c = colour(0) * (0.7 + 0.3 * uv.y);
    // a few stars
    let star = hash(floor(uv * vec2<f32>(aspect, 1.0) * 140.0));
    c = c + colour(4) * step(0.997, star) * 0.6;
    for (var k = 0; k < 3; k = k + 1) {
        let fk = f32(k);
        let x = uv.x * aspect * (1.2 + 0.4 * fk);
        let curve = 0.28 + 0.12 * fk + 0.08 * sin(x * 1.3 + t * (1.0 + fk * 0.3) + fk)
            + 0.05 * fbm(vec2<f32>(x * 0.8, t + fk));
        let level = band(0.15 + 0.3 * fk + 0.2 * uv.x);
        let d = uv.y - curve;
        // curtains hang down from the curve: rays, brighter at the curve
        let rays = 0.55 + 0.45 * fbm(vec2<f32>(x * 6.0, t * 2.0 + fk * 3.0));
        let below = exp(-max(d, 0.0) * (5.0 - 3.0 * level)) * step(0.0, d);
        let above = exp(-max(-d, 0.0) * 40.0);
        let light = (below * rays + above * 0.6) * (0.25 + 0.9 * level + 0.3 * u.t.y);
        c = c + mix(colour(1), colour(2 + k % 2), uv.y) * light * 0.6;
    }
    return c;
}

fn tunnel(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x;
    let r = max(length(p), 1e-3);
    let a = atan2(p.y, p.x);
    let depth = 0.35 / r + t * (0.6 + 1.4 * u.t.w);
    let turn = a / TAU + 0.15 * sin(depth * 0.7 + u.m.x);
    let rings = 0.5 + 0.5 * sin(depth * 6.0);
    let stripes = 0.5 + 0.5 * sin(turn * TAU * 8.0 + depth);
    // (folded round the circle: no seam where the angle wraps)
    let level = band(abs(2.0 * fract(turn) - 1.0));
    let light = rings * stripes * (0.4 + 0.8 * level) + 0.25 * u.t.y * rings;
    let fog = clamp(r * 2.2, 0.0, 1.0);
    return mix(colour(0), cycle(depth * 0.05 + level * 0.3) * (0.3 + light), fog) + colour(4) * pow(rings * stripes, 8.0) * fog * 0.5;
}

fn julia(p: vec2<f32>) -> vec3<f32> {
    // c along the edge of the Mandelbrot set's cardioid, a little inside:
    // a connected, intricate set at every point of the way
    let t = u.t.x * 0.07 + u.m.x * 0.05;
    let e = vec2<f32>(cos(t), sin(t));
    let e2 = vec2<f32>(cos(2.0 * t), sin(2.0 * t));
    let c = (e * 0.5 - e2 * 0.25) * (0.985 + 0.012 * u.t.w);
    var z = p * (1.35 - 0.15 * u.t.y);
    var i = 0;
    for (; i < 96; i = i + 1) {
        z = vec2<f32>(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
        if dot(z, z) > 64.0 {
            break;
        }
    }
    if i >= 96 {
        return mix(colour(0), colour(1), 0.15 + 0.2 * u.t.y);
    }
    let smooth_i = max(f32(i) - log2(log2(dot(z, z))) + 4.0, 0.0);
    let v = log2(1.0 + smooth_i) / log2(97.0);
    return palette(clamp(v * (0.9 + 0.25 * u.t.z), 0.0, 1.0));
}

fn reaction(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(data));
    let v = textureSampleLevel(data, data_sampler, uv, 0.0).r;
    let dx = textureSampleLevel(data, data_sampler, uv + vec2<f32>(1.0 / size.x, 0.0), 0.0).r - v;
    let dy = textureSampleLevel(data, data_sampler, uv + vec2<f32>(0.0, 1.0 / size.y), 0.0).r - v;
    let light = clamp(0.6 + 3.0 * (dx - dy), 0.0, 1.4);
    return palette(smoothstep(0.05, 0.6, v) * 0.95) * light;
}

fn ripples(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let q = vec2<f32>(uv.x * aspect, uv.y);
    var h = 0.0;
    var slope = vec2<f32>(0.0);
    for (var k = 0; k < 8; k = k + 1) {
        let d = u.drops[k];
        if d.w <= 0.0 {
            continue;
        }
        let centre = vec2<f32>(d.x * aspect, d.y);
        let v = q - centre;
        let r = length(v);
        let front = r - d.z * 0.35;
        let wave = sin(front * 60.0) * exp(-front * front * 90.0) * exp(-d.z * 1.1) * d.w;
        h = h + wave;
        slope = slope + v / max(r, 1e-3) * cos(front * 60.0) * exp(-front * front * 90.0) * exp(-d.z * 1.1) * d.w;
    }
    let ambient = 0.5 + 0.5 * sin(q.x * 7.0 + u.t.x * 0.7) * sin(q.y * 9.0 - u.t.x * 0.5);
    let base = mix(colour(0), colour(1), 0.25 + 0.2 * ambient * u.t.z);
    let light = clamp(dot(slope, vec2<f32>(0.6, -0.8)), -1.0, 1.0);
    return base + cycle(u.t.x * 0.03 + h * 0.2) * max(light, 0.0) * 0.9 + colour(4) * pow(max(light, 0.0), 6.0) * 0.6 - colour(0) * max(-light, 0.0) * 0.4;
}

fn moire(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x * 0.4;
    let a = vec2<f32>(0.35 * sin(t), 0.25 * cos(t * 0.8));
    let b = vec2<f32>(0.35 * sin(t * 0.7 + 2.0), 0.25 * cos(t * 1.1 + 1.0));
    let f1 = 40.0 + 30.0 * u.t.w;
    let f2 = 42.0 + 30.0 * u.m.y;
    let r1 = 0.5 + 0.5 * sin(length(p - a) * f1 - u.t.x * 2.0);
    let r2 = 0.5 + 0.5 * sin(length(p - b) * f2 + u.t.x * 1.5);
    let lines = 0.5 + 0.5 * sin((p.x * cos(t * 0.3) + p.y * sin(t * 0.3)) * (30.0 + 20.0 * u.m.z));
    let v = r1 * r2 + 0.35 * lines * r1;
    return mix(colour(0), cycle(v * 0.4 + t * 0.1), smoothstep(0.15, 0.9, v)) + colour(4) * u.t.y * 0.2 * v;
}

fn smoke(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let t = u.t.x * 0.25;
    let q = vec2<f32>(uv.x * aspect, uv.y) * 2.2;
    let warp = vec2<f32>(fbm(q + vec2<f32>(0.0, t * 2.0)), fbm(q + vec2<f32>(5.2, 1.3 + t * 2.0)));
    let warp2 = vec2<f32>(fbm(q + 3.0 * warp + vec2<f32>(1.7, 9.2) + t), fbm(q + 3.0 * warp + vec2<f32>(8.3, 2.8) - t));
    let d = fbm(q + 2.5 * warp2 + vec2<f32>(0.0, t * 3.0));
    let density = clamp((d - 0.25 + 0.35 * u.t.z) * (1.3 + u.t.y * 0.5), 0.0, 1.0);
    let shade = mix(colour(1), colour(3), clamp(length(warp2), 0.0, 1.0));
    return mix(colour(0), shade, density) + colour(4) * pow(density, 4.0) * 0.3;
}

fn mel(uv: vec2<f32>) -> vec3<f32> {
    // the newest column at the right edge, moving left between two steps
    let size = vec2<f32>(textureDimensions(data));
    let x = uv.x + u.face.z / size.x;
    let v = textureSampleLevel(data, data_sampler, vec2<f32>(x, uv.y), 0.0).r;
    return palette(v);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let px = in.position.xy;
    let uv = (px - u.rect.xy) / u.rect.zw;
    let aspect = u.rect.z / u.rect.w;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0) * 2.0;
    let face = i32(u.face.x + 0.5);
    var c: vec3<f32>;
    switch face {
        case 0: { c = mel(uv); }
        case 1: { c = plasma(p); }
        case 2: { c = fire(uv, aspect); }
        case 3: { c = metaballs(p); }
        case 4: { c = aurora(uv, aspect); }
        case 5: { c = tunnel(p); }
        case 6: { c = julia(p); }
        case 7: { c = reaction(uv); }
        case 8: { c = ripples(uv, aspect); }
        case 9: { c = moire(p); }
        default: { c = smoke(uv, aspect); }
    }
    c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    // the background's rounded corners
    let half = u.clip.zw * 0.5;
    let centre = u.clip.xy + half;
    let radius = u.m.w;
    let d = abs(px - centre) - half + vec2<f32>(radius);
    let outside = length(max(d, vec2<f32>(0.0))) - radius;
    let alpha = clamp(0.5 - outside, 0.0, 1.0);
    if u.face.y > 0.5 {
        c = to_linear(c);
    }
    return vec4<f32>(c * alpha, alpha);
}
