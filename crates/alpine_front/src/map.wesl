// The period map: paper, a brush-wash relief lit from the north-west, hatched rock, forest marked with
// little tree rings, brown contour lines (bold every 100 m), a faint 1 km grid and a double border.
// Then the same overlays as the 3D view: ground too steep to walk, and what the selected man can see.
//
// Heights are read exactly as the simulation reads them (same grid, same two triangles per square),
// so contour lines and steep ground are exactly the ground the soldiers walk on. Forest and the rest
// of the paint are decoration only.

import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;

struct TerrainSettings {
    flags: u32,
    contour_interval: f32,
    steep_cos: f32,
    cols: f32,
    rows: f32,
    min_x: f32,
    min_y: f32,
    spacing: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: TerrainSettings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var heights: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var relief_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var relief_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var viewshed_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var viewshed_sampler: sampler;

const FLAG_CONTOURS: u32 = 1u;
const FLAG_STEEP: u32 = 2u;
const FLAG_VIEWSHED: u32 = 4u;

// Colours are written as on screen (sRGB) and converted to the linear values the GPU blends in.
fn lin(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
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

fn fbm(p: vec2<f32>) -> f32 {
    var s = 0.0;
    var amp = 0.5;
    var q = p;
    for (var k = 0; k < 4; k++) {
        s += amp * noise(q);
        q = q * 2.03 + vec2<f32>(17.0, 9.0);
        amp *= 0.5;
    }
    return s;
}

fn h_at(c: i32, r: i32) -> f32 {
    return textureLoad(heights, vec2<i32>(c, r), 0).r;
}

struct Ground {
    h: f32,
    // Rise per metre east and north.
    grad: vec2<f32>,
}

// The simulation's `Terrain::height`: lower triangle (a, b, d) where fx >= fy, upper (a, d, c).
fn ground(x: f32, y: f32) -> Ground {
    let u = clamp((x - settings.min_x) / settings.spacing, 0.0, settings.cols - 1.0);
    let v = clamp((y - settings.min_y) / settings.spacing, 0.0, settings.rows - 1.0);
    let c = min(i32(floor(u)), i32(settings.cols) - 2);
    let r = min(i32(floor(v)), i32(settings.rows) - 2);
    let fx = u - f32(c);
    let fy = v - f32(r);
    let ha = h_at(c, r);
    let hd = h_at(c + 1, r + 1);
    var g: Ground;
    if fx >= fy {
        let hb = h_at(c + 1, r);
        g.h = ha + (hb - ha) * fx + (hd - hb) * fy;
        g.grad = vec2<f32>(hb - ha, hd - hb) / settings.spacing;
    } else {
        let hc = h_at(c, r + 1);
        g.h = ha + (hc - ha) * fy + (hd - hc) * fx;
        g.grad = vec2<f32>(hd - hc, hc - ha) / settings.spacing;
    }
    return g;
}

// A pattern size that stays fixed to the map while panning and only changes in steps when zooming.
fn ladder(metres: f32) -> f32 {
    return exp2(ceil(log2(metres)));
}

// 1 on a line through every whole value of `value`, `width` pixels wide.
fn line(value: f32, width: f32) -> f32 {
    let d = abs(fract(value - 0.5) - 0.5) / max(fwidth(value), 1e-5);
    return 1.0 - smoothstep(width * 0.5, width * 0.5 + 0.9, d);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let wp = in.world_position.xy;
    // Metres per screen pixel.
    let px = max(fwidth(wp.x), 1e-4);
    let g = ground(wp.x, wp.y);
    let up = inverseSqrt(1.0 + dot(g.grad, g.grad));
    let uv = vec2<f32>(
        ((wp.x - settings.min_x) / settings.spacing + 0.5) / settings.cols,
        ((wp.y - settings.min_y) / settings.spacing + 0.5) / settings.rows,
    );
    let relief = textureSample(relief_texture, relief_sampler, uv).rg;
    let view = textureSample(viewshed_texture, viewshed_sampler, uv).r;
    let contour_value = g.h / settings.contour_interval;
    let minor = line(contour_value, 0.8);
    let major = line(contour_value / 5.0, 1.5);
    let minor_spacing_px = 1.0 / max(fwidth(contour_value), 1e-5);
    let grid = max(line(wp.x / 1000.0, 0.7), line(wp.y / 1000.0, 0.7));

    // Paper: fibre grain fixed to the screen, slow stains fixed to the map.
    let grain = noise(in.position.xy * 0.8) - 0.5;
    let stain = fbm(wp / 1100.0) - 0.5;
    var rgb = lin(vec3<f32>(0.925, 0.885, 0.79)) * (1.0 + 0.05 * grain + 0.16 * stain);

    // Relief wash: shadowed slopes darken in a few uneven brush tones, sunlit slopes lighten.
    let flat_light = 0.7071;
    let shadow = clamp((flat_light - relief.r) / flat_light, 0.0, 1.0);
    let lit = clamp((relief.r - flat_light) / (1.0 - flat_light), 0.0, 1.0);
    let banded = floor(shadow * 5.0 + (noise(wp / 70.0) - 0.5) * 0.9) / 5.0;
    let wash = clamp(mix(shadow, banded, 0.5), 0.0, 1.0);
    rgb = rgb * mix(vec3<f32>(1.0), lin(vec3<f32>(0.6, 0.53, 0.46)), wash * 0.9);
    rgb = mix(rgb, lin(vec3<f32>(0.97, 0.95, 0.9)), lit * 0.25);

    // Forest below the tree line (decoration: the simulation has no forest yet), not on rock.
    let rock = smoothstep(0.82, 0.70, relief.g);
    let forest = smoothstep(1980.0, 1820.0, g.h) * (1.0 - rock);
    rgb = mix(rgb, rgb * lin(vec3<f32>(0.84, 0.9, 0.74)), forest);
    let cell = ladder(9.0 * px);
    let q = wp / cell;
    let id = floor(q);
    let jitter = vec2<f32>(hash(id), hash(id + vec2<f32>(17.0, 3.0))) * 0.5 + 0.25;
    let ring_px = length((fract(q) - jitter) * cell) / px;
    let tree = (1.0 - smoothstep(0.45, 1.0, abs(ring_px - 1.7))) * step(hash(id + vec2<f32>(5.0, 11.0)), forest * 0.8);
    rgb = mix(rgb, lin(vec3<f32>(0.24, 0.36, 0.2)), tree * 0.8);

    // Rock: diagonal hatching that thickens and breaks up like pen strokes.
    let hatch = ladder(4.0 * px);
    let along = dot(wp, vec2<f32>(0.7071, -0.7071)) / hatch;
    let stroke_px = abs(fract(along) - 0.5) * hatch / px;
    let strokes = (1.0 - smoothstep(0.35, 0.95, stroke_px)) * step(noise(wp / (hatch * 3.0)), rock * 1.15);
    let across = dot(wp, vec2<f32>(0.7071, 0.7071)) / hatch;
    let cross_px = abs(fract(across) - 0.5) * hatch / px;
    let steeper = smoothstep(0.62, 0.55, relief.g);
    let cross_strokes = (1.0 - smoothstep(0.35, 0.95, cross_px)) * step(noise(wp / (hatch * 3.0) + 7.0), steeper);
    rgb = mix(rgb, lin(vec3<f32>(0.27, 0.24, 0.21)), max(strokes, cross_strokes) * 0.7);

    // Overlays, as washes over the paper.
    if (settings.flags & FLAG_VIEWSHED) != 0u {
        if view < 0.25 {
            rgb = rgb * 0.88;
        } else {
            let seen = smoothstep(0.6, 0.9, view);
            let hidden = mix(rgb, lin(vec3<f32>(0.36, 0.45, 0.66)), 0.45);
            let shown = mix(rgb, lin(vec3<f32>(0.96, 0.8, 0.32)), 0.42);
            rgb = mix(hidden, shown, seen);
        }
    }
    if (settings.flags & FLAG_STEEP) != 0u && up < settings.steep_cos {
        rgb = mix(rgb, lin(vec3<f32>(0.78, 0.1, 0.05)), 0.5);
    }

    // The 1 km grid, in faint blue pencil.
    rgb = mix(rgb, lin(vec3<f32>(0.35, 0.45, 0.6)), grid * 0.35);

    // Contour lines in brown ink with uneven pen pressure. Minor lines fade out when they crowd
    // closer than about 4 pixels.
    if (settings.flags & FLAG_CONTOURS) != 0u {
        let pressure = 0.75 + 0.25 * noise(wp / 45.0);
        let crowd = smoothstep(2.5, 5.0, minor_spacing_px);
        let ink = max(minor * 0.6 * crowd, major) * pressure;
        rgb = mix(rgb, lin(vec3<f32>(0.5, 0.29, 0.13)), ink);
    }

    // Double border just inside the edge of the sheet.
    let max_x = settings.min_x + (settings.cols - 1.0) * settings.spacing;
    let max_y = settings.min_y + (settings.rows - 1.0) * settings.spacing;
    let edge = min(min(wp.x - settings.min_x, max_x - wp.x), min(wp.y - settings.min_y, max_y - wp.y)) / px;
    let outer = 1.0 - smoothstep(0.5, 1.5, abs(edge - 5.0) - 1.5);
    let inner = 1.0 - smoothstep(0.3, 1.0, abs(edge - 11.0));
    rgb = mix(rgb, lin(vec3<f32>(0.14, 0.11, 0.08)), max(outer, inner));
    rgb = mix(rgb, lin(vec3<f32>(0.925, 0.885, 0.79)), select(0.0, 0.85, edge < 3.0));

    return vec4<f32>(rgb, 1.0);
}
