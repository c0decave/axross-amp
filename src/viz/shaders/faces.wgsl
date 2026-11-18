// The pixel faces of axross-amp: one module, a face chosen by a uniform.
// Colours come in as the scheme's (sRGB, 0..1); `u.face.y` says whether
// the target wants linear values, `u.face.w` how opaque the face is (a
// crossfade). Every face is a function of the pixel, the time and the
// analysis; the feedback face also of its own last frame (`fs_feedback`
// draws the next one into a texture of its own), the mel spectrogram,
// the reaction-diffusion and the goniometer's phosphor of a picture the
// CPU keeps (the `data` texture). gpu.rs mirrors every function for the
// CPU.

struct U {
    rect: vec4<f32>,     // the face's area: x, y, width, height (pixels)
    clip: vec4<f32>,     // the rounded background: x, y, width, height
    t: vec4<f32>,        // time (s), beat pulse 0..1, energy, bass
    m: vec4<f32>,        // phase, middle, treble, corner radius (pixels)
    face: vec4<f32>,     // face, linear target (1), texture scroll, opacity
    colours: array<vec4<f32>, 5>, // background, low, middle, high, peak
    bands: array<vec4<f32>, 8>,   // 32 bands, bass to treble, 0..1
    extra: array<vec4<f32>, 8>,   // per face (the feedback's motion)
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

// Magma (matplotlib's perceptual map), a polynomial fit.
fn magma(t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    let c0 = vec3<f32>(-0.002136485, -0.000749655, -0.005386127);
    let c1 = vec3<f32>(0.2516605, 0.6775232, 2.494026);
    let c2 = vec3<f32>(8.353717, -3.577719, 0.3144679);
    let c3 = vec3<f32>(-27.66874, 14.26473, -13.64921);
    let c4 = vec3<f32>(52.17614, -27.94361, 12.94416);
    let c5 = vec3<f32>(-50.76852, 29.04658, 4.23415);
    let c6 = vec3<f32>(18.65571, -11.48977, -5.601962);
    return clamp(c0 + x * (c1 + x * (c2 + x * (c3 + x * (c4 + x * (c5 + x * c6))))), vec3<f32>(0.0), vec3<f32>(1.0));
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

fn rot(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

// -- the faces ----------------------------------------------------------------

// Liquid: domain-warped noise, flowing, with a sheen where it folds.
fn liquid(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x * (0.12 + 0.25 * u.t.z);
    let q = vec2<f32>(fbm(p * 1.1 + vec2<f32>(0.0, t)), fbm(p * 1.1 + vec2<f32>(5.2, 1.3 - t)));
    let r = vec2<f32>(
        fbm(p * 1.1 + 3.5 * q + vec2<f32>(1.7, 9.2) + 0.6 * t),
        fbm(p * 1.1 + 3.5 * q + vec2<f32>(8.3, 2.8) - 0.4 * t)
    );
    let v = fbm(p * 1.1 + (3.0 + 1.5 * u.t.w) * r);
    let base = palette(0.15 + 0.85 * smoothstep(0.2, 0.85, v));
    let fold = pow(clamp(length(r - q) * 1.2, 0.0, 1.0), 3.0);
    return base * (0.55 + 0.6 * v) + colour(4) * fold * (0.25 + 0.4 * u.t.y);
}

// Fire: flames rising from the bottom, as high as the bands below them.
fn fire(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let t = u.t.x;
    let y = 1.0 - uv.y;
    let x = uv.x * aspect;
    // (flames lick slowly: a fast flicker of red is what a photosensitive
    // eye cannot bear)
    let turb = fbm(vec2<f32>(x * 3.0, y * 2.0 - t * (1.0 + 0.6 * u.t.z)));
    let lick = fbm(vec2<f32>(x * 5.0 + 3.0, y * 3.0 - t * 1.6));
    let height = 0.5 + 0.4 * band(uv.x * 0.7) + 0.15 * u.t.y;
    let shape = 1.0 - y / max(height, 0.05);
    let heat = clamp(shape * 0.85 + (turb - 0.5) * 1.2 + (lick - 0.5) * 0.6 * (1.0 - y), 0.0, 1.0);
    // a fire's own colours: black, ember, flame, yellow, white
    let h = pow(heat, 1.4);
    // (embers orange, not a saturated red)
    var c = mix(vec3<f32>(0.02, 0.008, 0.006), vec3<f32>(0.55, 0.17, 0.03), smoothstep(0.0, 0.35, h));
    c = mix(c, vec3<f32>(0.95, 0.35, 0.02), smoothstep(0.3, 0.6, h));
    c = mix(c, vec3<f32>(1.0, 0.8, 0.25), smoothstep(0.55, 0.85, h));
    c = mix(c, vec3<f32>(1.0, 0.97, 0.85), smoothstep(0.85, 1.0, h));
    // the scheme's loud colour in the embers, a little
    return mix(c, c * (0.6 + 0.8 * colour(3)), 0.1) + colour(0) * (1.0 - smoothstep(0.0, 0.2, h)) * 0.6;
}

fn metaballs(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x * 0.6;
    var field = 0.0;
    var tint = vec3<f32>(0.0);
    for (var k = 0; k < 8; k = k + 1) {
        let fk = f32(k);
        let centre = vec2<f32>(
            1.3 * sin(t * (0.5 + 0.13 * fk) + fk * 1.7),
            0.6 * cos(t * (0.4 + 0.11 * fk) + fk * 2.3)
        );
        let level = band((fk + 0.5) / 8.0);
        let r = 0.14 + 0.16 * level + 0.05 * u.t.y;
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
    let star = hash(floor(uv * vec2<f32>(aspect, 1.0) * 140.0));
    c = c + colour(4) * step(0.997, star) * 0.6;
    // curtains: a bright foot that waves, light rising from it in rays
    // and fading upwards (not hills of light falling down)
    for (var k = 0; k < 3; k = k + 1) {
        let fk = f32(k);
        let x = uv.x * aspect * (1.0 + 0.3 * fk);
        let foot = 0.62 - 0.12 * fk + 0.07 * sin(x * 1.1 + t * (0.8 + 0.25 * fk) + fk)
            + 0.05 * fbm(vec2<f32>(x * 0.7, t + fk));
        let level = band(0.15 + 0.3 * fk + 0.2 * uv.x);
        let d = foot - uv.y;
        // (soft rays, broad: folds of a curtain, not a barcode)
        let ray = fbm(vec2<f32>(x * 3.5, t * 0.4 + fk * 3.0));
        let rays = 0.35 + 0.9 * ray;
        let up = exp(-max(d, 0.0) * (3.0 - 1.5 * level)) * step(0.0, d);
        let edge = exp(-abs(d) * 60.0);
        let light = (up * rays + edge * 0.8) * (0.3 + 0.9 * level + 0.2 * u.t.y);
        let tint = mix(colour(2), colour(3 - k % 2), clamp(d * 3.0, 0.0, 1.0));
        c = c + tint * light * 0.55;
    }
    return c;
}

// Tunnel: a dark tunnel of glowing lines coming at you - rings across it,
// ribs along it, each lit by its band - a light at its end on the beat.
fn tunnel(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x;
    let r = max(length(p), 1e-3);
    let a = atan2(p.y, p.x);
    let depth = 0.45 / r + t * (0.35 + 0.6 * u.t.w);
    let turn = a / TAU + 0.1 * sin(depth * 0.35 + u.m.x);
    // (folded round the circle: no seam where the angle wraps)
    let fold = abs(2.0 * fract(turn) - 1.0);
    let level = band(fold);
    // each line's distance in the picture's units: as thin far off as near
    let rr = fract(depth * 1.5);
    let ring_d = min(rr, 1.0 - rr) / 1.5 * r * r / 0.45;
    let ss = fract(turn * 10.0);
    let rib_d = min(ss, 1.0 - ss) / 10.0 * TAU * r;
    let ring = exp(-ring_d / 0.010);
    let rib = exp(-rib_d / 0.008);
    // the walls between: dark, a checker of the scheme's tint
    let checker = fract((floor(depth * 1.5) + floor(turn * 10.0)) * 0.5) * 2.0;
    let wall = mix(colour(0), cycle(floor(depth * 1.5) * 0.04 + fold * 0.4), 0.10 + 0.22 * level)
        * (0.75 + 0.25 * checker);
    let line_colour = cycle(floor(depth * 1.5) * 0.04 + fold * 0.4 + 0.15);
    let glow = ring * (0.35 + 1.1 * level + 0.5 * u.t.y) + rib * (0.2 + 0.8 * level);
    let c = wall + line_colour * glow;
    // into the dark far off, a light at the end
    let fog = smoothstep(0.03, 0.6, r);
    return mix(colour(0) * 0.6, c, fog) + colour(3) * exp(-r * 9.0) * (0.25 + 0.6 * u.t.y + 0.4 * u.t.z);
}

// Julia set: smooth escape colours and glowing orbit traps.
fn julia(p: vec2<f32>) -> vec3<f32> {
    // c along the edge of the Mandelbrot set's cardioid, a little inside
    let t = u.t.x * 0.07 + u.m.x * 0.05;
    let e = vec2<f32>(cos(t), sin(t));
    let e2 = vec2<f32>(cos(2.0 * t), sin(2.0 * t));
    let c = (e * 0.5 - e2 * 0.25) * (0.985 + 0.012 * u.t.w);
    var z = p * (1.25 - 0.1 * u.t.y);
    var trap = 1e9;
    var i = 0;
    for (; i < 128; i = i + 1) {
        z = vec2<f32>(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
        trap = min(trap, length(z - vec2<f32>(0.25 * sin(u.t.x * 0.3), 0.0)));
        if dot(z, z) > 256.0 {
            break;
        }
    }
    let trapped = exp(-trap * 6.0);
    if i >= 128 {
        return mix(colour(0), colour(1), 0.2 + 0.5 * trapped);
    }
    let smooth_i = max(f32(i) + 1.0 - log2(log2(dot(z, z)) * 0.5), 0.0);
    let v = clamp(sqrt(smooth_i / 128.0) * (0.95 + 0.25 * u.t.z), 0.0, 1.0);
    return palette(v) + cycle(trap * 0.5 + u.t.x * 0.02) * trapped * 0.6;
}

fn reaction(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(data));
    let v = textureSampleLevel(data, data_sampler, uv, 0.0).r;
    let dx = textureSampleLevel(data, data_sampler, uv + vec2<f32>(1.0 / size.x, 0.0), 0.0).r - v;
    let dy = textureSampleLevel(data, data_sampler, uv + vec2<f32>(0.0, 1.0 / size.y), 0.0).r - v;
    let light = clamp(0.6 + 3.0 * (dx - dy), 0.0, 1.4);
    return palette(smoothstep(0.05, 0.6, v) * 0.95) * light;
}

// The feedback face shown: its own picture (drawn by fs_feedback).
fn shown(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(data, data_sampler, uv, 0.0).rgb;
}

// Moiré: rings against rings, antialiased - where they get finer than a
// pixel they fade to their mean instead of aliasing.
fn wave(phase: f32, per_pixel: f32) -> f32 {
    let aa = clamp(1.0 - per_pixel / 3.0, 0.0, 1.0);
    return 0.5 + 0.5 * sin(phase) * aa;
}

fn moire(p: vec2<f32>, pixel: f32) -> vec3<f32> {
    let t = u.t.x * 0.4;
    let a = vec2<f32>(0.35 * sin(t), 0.25 * cos(t * 0.8));
    let b = vec2<f32>(0.35 * sin(t * 0.7 + 2.0), 0.25 * cos(t * 1.1 + 1.0));
    // (dense rings, interfering - slower than they were, and no brighter
    // than 0.09 above the ground: no pattern hazard)
    let f1 = 26.0 + 10.0 * u.t.w;
    let f2 = 28.0 + 10.0 * u.m.y;
    let f3 = 18.0 + 8.0 * u.m.z;
    let r1 = wave(length(p - a) * f1 - u.t.x * 0.9, f1 * pixel);
    let r2 = wave(length(p - b) * f2 + u.t.x * 0.7, f2 * pixel);
    let lines = wave((p.x * cos(t * 0.3) + p.y * sin(t * 0.3)) * f3, f3 * pixel);
    let v = r1 * r2 + 0.35 * lines * r1;
    let ground = colour(0);
    let c = mix(ground, cycle(v * 0.4 + t * 0.1), smoothstep(0.15, 0.9, v));
    let over = luma(c) - luma(ground);
    if over > 0.09 {
        return mix(ground, c, 0.09 / over);
    }
    return c;
}

fn no_red(c: vec3<f32>) -> vec3<f32> {
    var out = c;
    for (var i = 0; i < 8; i++) {
        let l = pow(max(out, vec3<f32>(0.0)), vec3<f32>(2.2));
        let sum = l.x + l.y + l.z;
        if sum <= 0.0 || l.x / sum < 0.75 {
            break;
        }
        out = mix(out, vec3<f32>(1.0, 0.93, 0.84), 0.12);
    }
    return out;
}

fn luma(c: vec3<f32>) -> f32 {
    let l = pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
    return 0.2126 * l.x + 0.7152 * l.y + 0.0722 * l.z;
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
    return magma(v);
}

// The waterfall at high resolution: the newest row at the bottom, moving
// up between two steps; axross's waterfall colours.
fn spectrogram(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(data));
    let y = uv.y + u.face.z / size.y;
    let v = textureSampleLevel(data, data_sampler, vec2<f32>(uv.x, y), 0.0).r;
    if v < 0.15 {
        return mix(colour(0), colour(1), v / 0.15);
    } else if v < 0.6 {
        return mix(colour(1), colour(2), (v - 0.15) / 0.45);
    }
    return mix(colour(2), colour(3), min((v - 0.6) / 0.4, 1.0));
}

// The goniometer's phosphor: what the beam drew, fading.
fn phosphor(uv: vec2<f32>) -> vec3<f32> {
    let v = textureSampleLevel(data, data_sampler, uv, 0.0).r;
    return magma(pow(v, 0.6) * 0.95);
}

// Flyover: a landscape the bass lifts, flown over towards a sun.
fn terrain_height(x: f32, z: f32) -> f32 {
    let ridge = band(clamp(abs(x) * 0.08, 0.0, 1.0));
    return fbm(vec2<f32>(x * 0.18, z * 0.18)) * 2.2 + ridge * (0.8 + 1.2 * u.t.w) * smoothstep(1.0, 6.0, abs(x));
}

fn flyover(uv: vec2<f32>, aspect: f32, steps: i32) -> vec3<f32> {
    let p = vec2<f32>((uv.x - 0.5) * aspect, 0.5 - uv.y);
    let t = u.t.x * (1.5 + 1.5 * u.t.z);
    let origin = vec3<f32>(0.0, 2.6, t);
    let dir = normalize(vec3<f32>(p.x, p.y - 0.18, 1.0));
    // the night: dark above, a glow of the loud colour at the horizon
    let horizon = mix(colour(0), colour(3), 0.35);
    let sky = mix(horizon, colour(0) * 0.7, clamp((p.y + 0.12) * 2.2, 0.0, 1.0));
    let sun_dir = normalize(vec3<f32>(0.0, 0.12, 1.0));
    let facing = max(dot(dir, sun_dir), 0.0);
    let sun = smoothstep(0.9975, 0.9985, facing);
    var d = 0.1;
    var before = 0.1;
    var hit = false;
    var pos = origin;
    for (var k = 0; k < steps; k = k + 1) {
        pos = origin + dir * d;
        let h = terrain_height(pos.x, pos.z);
        let gap = pos.y - h;
        if gap < 0.01 * d {
            hit = true;
            break;
        }
        before = d;
        d = d + max(gap * 0.45, 0.02 * d);
        if d > 40.0 {
            break;
        }
    }
    if hit {
        // (the surface itself, where the height above it is 0, found from
        // the last two steps by the secant: no stairs of the steps' size)
        var a0 = before;
        var a1 = d;
        var q = origin + dir * a0;
        var g0 = q.y - terrain_height(q.x, q.z);
        q = origin + dir * a1;
        var g1 = q.y - terrain_height(q.x, q.z);
        for (var k = 0; k < 4; k = k + 1) {
            if abs(g1 - g0) < 1e-5 {
                break;
            }
            let a2 = clamp(a1 - g1 * (a1 - a0) / (g1 - g0), before, d * 1.1);
            a0 = a1;
            g0 = g1;
            a1 = a2;
            q = origin + dir * a1;
            g1 = q.y - terrain_height(q.x, q.z);
        }
        d = a1;
        pos = origin + dir * d;
    }
    if !hit {
        let disc = mix(colour(3), mix(colour(3), colour(4), 0.6), clamp((p.y - 0.05) * 6.0, 0.0, 1.0));
        return mix(sky, disc, sun) + colour(3) * pow(facing, 40.0) * (0.35 + 0.4 * u.t.y);
    }
    let e = 0.05;
    let n = normalize(vec3<f32>(
        terrain_height(pos.x - e, pos.z) - terrain_height(pos.x + e, pos.z),
        2.0 * e,
        terrain_height(pos.x, pos.z - e) - terrain_height(pos.x, pos.z + e)
    ));
    let light = clamp(dot(n, normalize(vec3<f32>(0.3, 0.8, 0.6))), 0.0, 1.0);
    let height = clamp(pos.y / 3.0, 0.0, 1.0);
    // dark land, lit from the side, the quiet colour on its heights
    let land = mix(colour(0), colour(1), 0.22 + 0.45 * height) * (0.3 + 0.8 * light);
    // a grid over it (lines as wide as a pixel far off), lit by the bands
    let g = fract(vec2<f32>(pos.x, pos.z) * 0.5);
    let gd = min(min(g.x, 1.0 - g.x), min(g.y, 1.0 - g.y)) * 2.0;
    let grid = (1.0 - smoothstep(0.0, 0.008 + 0.005 * d, gd)) * exp(-d * 0.05);
    let level = band(clamp(abs(pos.x) * 0.08, 0.0, 1.0));
    let lines = mix(colour(2), colour(3), level) * grid * (0.55 + 0.9 * level + 0.4 * u.t.y);
    // ridges catch the sun
    let rim = colour(3) * pow(clamp(1.0 - n.y, 0.0, 1.0), 2.0) * 0.5 * (0.5 + level);
    let fog = 1.0 - exp(-d * 0.09);
    return mix(land + lines + rim, horizon, fog);
}

// Crystals: cells of a moving Voronoi pattern, each lit by its band, their
// edges glowing on the beat.
fn crystals(p: vec2<f32>) -> vec3<f32> {
    let t = u.t.x * 0.7;
    let q = p * 3.2;
    let cell = floor(q);
    var f1 = 9.0;
    var f2 = 9.0;
    var id = vec2<f32>(0.0);
    for (var j = -1; j <= 1; j = j + 1) {
        for (var i = -1; i <= 1; i = i + 1) {
            let g = cell + vec2<f32>(f32(i), f32(j));
            let o = vec2<f32>(hash(g), hash(g + 17.3));
            let at = g + 0.5 + 0.4 * sin(t + TAU * o);
            let dist = length(q - at);
            if dist < f1 {
                f2 = f1;
                f1 = dist;
                id = g;
            } else if dist < f2 {
                f2 = dist;
            }
        }
    }
    let which = hash(id + 3.1);
    let level = band(which);
    let edge = 1.0 - smoothstep(0.0, 0.08 + 0.05 * u.t.y, f2 - f1);
    // (each cell lit evenly, its edges softer: cells of a size, each light
    // in the middle and dark at the rim, lay in a grating over 47 %)
    let raw = palette(0.2 + 0.75 * level);
    let k = pow(max(luma(palette(0.55)), 0.02) / max(luma(raw), 1e-4), 1.0 / 2.2);
    let face = no_red(min(raw * k, vec3<f32>(1.0))) * (0.93 + 0.07 * (1.0 - f1));
    let facet = 0.5 + 0.5 * dot(normalize(q - id - 0.5 + 1e-4), vec2<f32>(0.6, -0.8));
    return face * (0.96 + 0.04 * facet) + colour(4) * edge * (0.02 + 0.04 * level);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

fn face_colour(uv: vec2<f32>, aspect: f32, pixel: f32) -> vec3<f32> {
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0) * 2.0;
    let face = i32(u.face.x + 0.5);
    var c: vec3<f32>;
    switch face {
        case 0: { c = mel(uv); }
        case 1: { c = liquid(p); }
        case 2: { c = fire(uv, aspect); }
        case 3: { c = metaballs(p); }
        case 4: { c = aurora(uv, aspect); }
        case 5: { c = tunnel(p); }
        case 6: { c = julia(p); }
        case 7: { c = reaction(uv); }
        case 8: { c = shown(uv); }
        case 9: { c = moire(p, pixel); }
        case 10: { c = smoke(uv, aspect); }
        case 11: { c = flyover(uv, aspect, 96); }
        case 12: { c = crystals(p); }
        case 15: { c = spectrogram(uv); }
        default: { c = phosphor(uv); }
    }
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The data texture at uv by a cubic B-spline: four bilinear fetches, each
// weighing two texels on each axis.
fn smooth_sample(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(data));
    let p = uv * size - 0.5;
    let i = floor(p);
    let f = p - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - f) * (1.0 - f) * (1.0 - f) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let at0 = (i - 1.0 + w1 / g0 + 0.5) / size;
    let at1 = (i + 1.0 + w3 / g1 + 0.5) / size;
    let s00 = textureSampleLevel(data, data_sampler, vec2<f32>(at0.x, at0.y), 0.0);
    let s10 = textureSampleLevel(data, data_sampler, vec2<f32>(at1.x, at0.y), 0.0);
    let s01 = textureSampleLevel(data, data_sampler, vec2<f32>(at0.x, at1.y), 0.0);
    let s11 = textureSampleLevel(data, data_sampler, vec2<f32>(at1.x, at1.y), 0.0);
    return (s00 * g0.x + s10 * g1.x) * g0.y + (s01 * g0.x + s11 * g1.x) * g1.y;
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let px = in.position.xy;
    let uv = (px - u.rect.xy) / u.rect.zw;
    // (nothing outside the face's own rectangle: the mel spectrogram's
    // left edge, stretched over its labels' margin, lay under them)
    if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 {
        return vec4<f32>(0.0);
    }
    let aspect = u.rect.z / u.rect.w;
    let face = i32(u.face.x + 0.5);
    // the colour (premultiplied by `a`, how much of it there is)
    var c: vec3<f32>;
    var a = 1.0;
    if face == 14 {
        // a face drawn smaller (a slow GPU), shown scaled up - smoothly (a
        // cubic B-spline: no blocks, no diamonds of the plain bilinear)
        let t = smooth_sample(uv);
        c = t.rgb;
        a = t.a;
    } else {
        // (a pixel in the faces' units: 2 across the height)
        c = face_colour(uv, aspect, 2.0 / u.rect.w);
        // (the phosphor lies over the face's ground: dark where nothing glows)
        if face == 13 {
            a = clamp(textureSampleLevel(data, data_sampler, uv, 0.0).r * 6.0, 0.0, 1.0);
            c = c * a;
        } else {
            // (a little noise below a step of 8 bits: no bands in slow
            // gradients)
            c = c + vec3<f32>((hash(floor(px)) - 0.5) / 255.0);
        }
    }
    // the background's rounded corners
    let half = u.clip.zw * 0.5;
    let centre = u.clip.xy + half;
    let radius = u.m.w;
    let d = abs(px - centre) - half + vec2<f32>(radius);
    let outside = length(max(d, vec2<f32>(0.0))) - radius;
    let mask = clamp(0.5 - outside, 0.0, 1.0) * u.face.w;
    if u.face.y > 0.5 {
        c = to_linear(c);
    }
    return vec4<f32>(c * mask, a * mask);
}

// -- the feedback face (MilkDrop's way) ------------------------------------
// Its last frame (`data`), turned, zoomed and swirled a little, fading,
// and a new layer over it: a ring of the bands and sparks on the beat.

// One of the presets' waves: 0 a ring of the bands, 1 a scope's line,
// 2 spiral arms, 3 orbiting lights.
fn wave_layer(kind: i32, p: vec2<f32>, aspect: f32, t: f32) -> vec3<f32> {
    let r = length(p);
    let a = atan2(p.y, p.x);
    let hue = u.extra[2].x;
    if kind == 1 {
        let x = clamp(abs(p.x) / (0.5 * aspect), 0.0, 1.0);
        let level = band(x);
        let y = (0.06 + 0.22 * level) * sin(p.x * 9.0 + t * 2.0);
        let line = exp(-abs(p.y - y) * (150.0 - 60.0 * u.t.y)) * (0.4 + 0.9 * level);
        return cycle(p.x * 0.4 + t * hue) * line;
    } else if kind == 2 {
        let level = band(abs(a) / 3.14159);
        let arms = 0.5 + 0.5 * cos(3.0 * a - log(max(r, 1e-3)) * 5.0 + t * 1.5);
        let lit = pow(arms, 24.0) * smoothstep(0.03, 0.12, r) * exp(-r * 2.5) * (0.5 + 1.2 * level);
        return cycle(a / TAU + r + t * hue) * lit;
    } else if kind == 3 {
        var c = vec3<f32>(0.0);
        for (var k = 0; k < 5; k = k + 1) {
            let fk = f32(k);
            let level = band((fk + 0.5) / 5.0);
            let turn = t * (0.6 + 0.1 * fk) + fk * TAU / 5.0;
            let centre = vec2<f32>(cos(turn), sin(turn)) * (0.22 + 0.15 * level);
            let d = length(p - centre);
            c = c + cycle(fk / 5.0 + t * hue) * exp(-d * (70.0 - 30.0 * u.t.y)) * (0.5 + level);
        }
        return c;
    }
    let level = band(abs(a) / 3.14159);
    let ring = 0.18 + 0.22 * level + 0.06 * u.t.w;
    let line = exp(-abs(r - ring) * (160.0 - 80.0 * u.t.y)) * (0.4 + 0.9 * level);
    return cycle(a / TAU + t * hue) * line;
}

fn feedback(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let t = u.t.x;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    // the motion: u.extra[0] = turn, zoom, swirl, decay; the preset:
    // u.extra[1] = its wave, the last one's, how far between, the warp's
    // fineness; u.extra[2] = the colours' turn, the mirror
    let motion = u.extra[0];
    let look = u.extra[1];
    var q = rot(p, motion.x) * motion.y;
    q = q + motion.z * vec2<f32>(sin(q.y * look.w + t), cos(q.x * look.w - t * 0.8));
    q = mix(q, vec2<f32>(abs(q.x), q.y), u.extra[2].y);
    let src = q / vec2<f32>(aspect, 1.0) + 0.5;
    var prev = textureSampleLevel(data, data_sampler, src, 0.0).rgb;
    // (outside the last picture: nothing)
    let inside = step(0.0, src.x) * step(src.x, 1.0) * step(0.0, src.y) * step(src.y, 1.0);
    prev = prev * inside * motion.w;
    // a slow turn of the colours as they fade
    prev = vec3<f32>(prev.r * 0.985 + prev.b * 0.012, prev.g * 0.99 + prev.r * 0.008, prev.b * 0.985 + prev.g * 0.012);
    let wave = mix(
        wave_layer(i32(look.x + 0.5), p, aspect, t),
        wave_layer(i32(look.y + 0.5), p, aspect, t),
        look.z
    );
    let spark = smoothstep(0.996 - 0.01 * u.t.y, 1.0, hash(floor(uv * vec2<f32>(aspect, 1.0) * 90.0) + floor(t * 8.0)));
    let layer = wave + colour(4) * spark * u.t.y;
    // (a floor below which it is black: 8 bits never fade to 0 by themselves)
    return clamp(max(prev - vec3<f32>(1.5 / 255.0), vec3<f32>(0.0)) + layer, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_feedback(in: Out) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(data));
    let uv = in.position.xy / size;
    return vec4<f32>(feedback(uv, size.x / size.y), 1.0);
}
