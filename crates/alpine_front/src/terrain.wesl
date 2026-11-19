// Terrain shader: ground colour from height and steepness, lit by Bevy's standard lighting, then the
// debug overlays: contour lines, ground too steep to walk, and what the selected observer can see.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

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

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> settings: TerrainSettings;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var viewshed_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var viewshed_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var snow_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var snow_sampler: sampler;

const FLAG_CONTOURS: u32 = 1u;
const FLAG_STEEP: u32 = 2u;
const FLAG_VIEWSHED: u32 = 4u;
const FLAG_SNOW: u32 = 8u;

// Summer ground: forest green low down, meadow higher up, grey rock where steep.
fn ground_colour(height: f32, up: f32) -> vec3<f32> {
    let forest = vec3<f32>(0.035, 0.07, 0.025);
    let meadow = vec3<f32>(0.11, 0.16, 0.05);
    let high_grass = vec3<f32>(0.16, 0.16, 0.08);
    let rock = vec3<f32>(0.22, 0.21, 0.19);
    var c = mix(forest, meadow, smoothstep(1650.0, 1950.0, height));
    c = mix(c, high_grass, smoothstep(2150.0, 2400.0, height));
    // up = cosine of the slope: 1 on flat ground, 0.82 at 35 degrees, 0.71 at 45.
    return mix(c, rock, smoothstep(0.86, 0.74, up));
}

// 1 on a contour line, fading to 0 within about `width` pixels.
fn contour(value: f32, width: f32) -> f32 {
    let d = abs(fract(value - 0.5) - 0.5) / max(fwidth(value), 1e-5);
    return 1.0 - clamp(d / width, 0.0, 1.0);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let wp = in.world_position.xyz;
    // The flat triangle's own normal: exactly the slope the simulation uses.
    let face = normalize(cross(dpdx(wp), dpdy(wp)));
    let up = abs(face.y);
    // Colour follows the smooth slope, so the ground doesn't speckle triangle by triangle.
    var colour = ground_colour(wp.y, abs(normalize(in.world_normal).y));
    let x_cells = (wp.x - settings.min_x) / settings.spacing;
    let y_cells = (-wp.z - settings.min_y) / settings.spacing;
    let uv = vec2<f32>((x_cells + 0.5) / settings.cols, (y_cells + 0.5) / settings.rows);
    // Fresh snow whitens the ground by its depth; packed tracks show as grey-blue paths.
    if (settings.flags & FLAG_SNOW) != 0u {
        let snow = textureSample(snow_texture, snow_sampler, uv).rg;
        colour = mix(colour, vec3<f32>(0.80, 0.82, 0.86), clamp(snow.r * 3.0, 0.0, 0.9));
        colour = mix(colour, vec3<f32>(0.22, 0.24, 0.30), smoothstep(0.1, 0.5, snow.g) * 0.85);
    }
    pbr_input.material.base_color = vec4<f32>(colour, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    var rgb = out.color.rgb;

    if (settings.flags & FLAG_STEEP) != 0u && up < settings.steep_cos {
        rgb = mix(rgb, vec3<f32>(0.6, 0.02, 0.01), 0.6);
    }

    if (settings.flags & FLAG_VIEWSHED) != 0u {
        let v = textureSample(viewshed_texture, viewshed_sampler, uv).r;
        if v < 0.25 {
            rgb = rgb * 0.55;
        } else {
            let seen = smoothstep(0.6, 0.9, v);
            let hidden = rgb * vec3<f32>(0.35, 0.4, 0.6);
            let shown = mix(rgb, vec3<f32>(0.9, 0.75, 0.25), 0.35) * 1.25;
            rgb = mix(hidden, shown, seen);
        }
    }

    if (settings.flags & FLAG_CONTOURS) != 0u {
        let minor = contour(wp.y / settings.contour_interval, 0.8);
        let major = contour(wp.y / (settings.contour_interval * 5.0), 1.3);
        rgb = mix(rgb, rgb * 0.35, max(minor * 0.55, major));
    }

    out.color = vec4<f32>(rgb, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
