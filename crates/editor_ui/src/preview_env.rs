//! The environment the material preview is judged against (M4-ACCEPTANCE D11).
//!
//! A metal surface is a mirror: with nothing around it, it renders black and
//! `metallic` becomes a slider with no visible effect. Roughness has the same
//! problem in weaker form — under a single directional light it only moves one
//! specular dot, when what it actually controls is how sharply the surroundings
//! are reflected. Two of the six parameters the panel exposes were therefore
//! unjudgeable.
//!
//! So the preview gets a studio to stand in. The cubemap here is generated
//! rather than shipped: an asset would be a binary blob in the repo needing a
//! licence and a pipeline, and what is wanted is a neutral room, not a
//! photograph. Bevy filters it on the GPU (`GeneratedEnvironmentMapLight`), so
//! roughness gets a real prefiltered mip chain rather than one flat reflection.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

/// Face size. Small on purpose: it is filtered into mips immediately, and a
/// studio backdrop has no detail worth more than this.
const FACE: u32 = 64;

/// The generated studio, kept alive for as long as the editor runs.
#[derive(Resource)]
pub(crate) struct PreviewEnvironment(pub Handle<Image>);

/// Direction of the texel at `uv` on cubemap `face`, in the wgpu face order
/// (+X, -X, +Y, -Y, +Z, -Z).
fn face_direction(face: u32, uv: Vec2) -> Vec3 {
    // uv in [-1, 1], v flipped: cubemaps address top-left down.
    let (u, v) = (uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let direction = match face {
        0 => Vec3::new(1.0, v, -u),
        1 => Vec3::new(-1.0, v, u),
        2 => Vec3::new(u, 1.0, -v),
        3 => Vec3::new(u, -1.0, v),
        4 => Vec3::new(u, v, 1.0),
        _ => Vec3::new(-u, v, -1.0),
    };
    direction.normalize()
}

/// What the room looks like in a given direction: a bright ceiling, a mid
/// horizon, a darker floor, and one broad key light high to the left — the
/// arrangement a turntable render uses, because it is the one that makes both
/// the shape and the finish of a surface readable at a glance.
fn studio_radiance(direction: Vec3) -> [f32; 3] {
    let up = direction.y;
    // Vertical gradient: floor → horizon → ceiling.
    let base = if up >= 0.0 {
        let t = up.powf(0.7);
        [0.32 + 0.60 * t, 0.34 + 0.60 * t, 0.38 + 0.58 * t]
    } else {
        let t = (-up).powf(0.6);
        [0.26 - 0.16 * t, 0.25 - 0.16 * t, 0.24 - 0.15 * t]
    };
    // A soft key: broad enough to read as a source with an edge, which is what
    // roughness blurs. A point-like highlight would tell you almost nothing.
    let key_direction = Vec3::new(-0.45, 0.72, 0.53).normalize();
    let key = direction.dot(key_direction).max(0.0).powf(6.0);
    // And a dimmer fill opposite it, so the dark side is not a void.
    let fill_direction = Vec3::new(0.62, 0.28, -0.73).normalize();
    let fill = direction.dot(fill_direction).max(0.0).powf(3.0) * 0.25;
    [
        (base[0] + key + fill).clamp(0.0, 1.0),
        (base[1] + key * 0.98 + fill).clamp(0.0, 1.0),
        (base[2] + key * 0.94 + fill * 1.05).clamp(0.0, 1.0),
    ]
}

pub(crate) fn build_studio_cubemap() -> Image {
    let mut data: Vec<u8> = Vec::with_capacity((FACE * FACE * 6 * 4) as usize);
    for face in 0..6 {
        for y in 0..FACE {
            for x in 0..FACE {
                let uv = Vec2::new(
                    (x as f32 + 0.5) / FACE as f32,
                    (y as f32 + 0.5) / FACE as f32,
                );
                let radiance = studio_radiance(face_direction(face, uv));
                for channel in radiance {
                    // sRGB-encoded: the format says so, and the shader decodes.
                    let encoded = if channel <= 0.003_130_8 {
                        channel * 12.92
                    } else {
                        1.055 * channel.powf(1.0 / 2.4) - 0.055
                    };
                    data.push((encoded.clamp(0.0, 1.0) * 255.0).round() as u8);
                }
                data.push(255);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: FACE,
            height: FACE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // Six layers become a CUBE only if the view says so.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

pub(crate) fn setup_preview_environment(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let handle = images.add(build_studio_cubemap());
    commands.insert_resource(PreviewEnvironment(handle));
}

#[cfg(test)]
mod tests {
    use super::*;

    // The six faces have to actually face six ways, or the room is inside out
    // and every reflection is wrong in a way that is hard to see and easy to
    // ship.
    #[test]
    fn each_face_points_along_its_own_axis() {
        let centre = Vec2::splat(0.5);
        let expected = [
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::Z,
            Vec3::NEG_Z,
        ];
        for (face, axis) in expected.into_iter().enumerate() {
            let direction = face_direction(face as u32, centre);
            assert!(
                direction.abs_diff_eq(axis, 1e-5),
                "face {face} centre points {direction:?}, expected {axis:?}"
            );
        }
    }

    // Up is brighter than down: a room lit from the ceiling is the whole reason
    // a sphere reads as a sphere.
    #[test]
    fn the_ceiling_is_brighter_than_the_floor() {
        let ceiling = studio_radiance(Vec3::Y);
        let floor = studio_radiance(Vec3::NEG_Y);
        assert!(
            ceiling[0] > floor[0] && ceiling[1] > floor[1] && ceiling[2] > floor[2],
            "ceiling {ceiling:?} floor {floor:?}"
        );
    }

    // Bevy panics if the source is not square and power-of-two, and the six
    // layers only read as a cube when the view descriptor says Cube.
    #[test]
    fn the_cubemap_is_shaped_the_way_bevy_demands() {
        let image = build_studio_cubemap();
        let size = image.texture_descriptor.size;
        assert_eq!(size.width, size.height, "square");
        assert!(size.width.is_power_of_two(), "power of two");
        assert_eq!(size.depth_or_array_layers, 6, "six faces");
        assert_eq!(
            image
                .texture_view_descriptor
                .as_ref()
                .and_then(|view| view.dimension),
            Some(TextureViewDimension::Cube),
            "viewed as a cube"
        );
        assert_eq!(
            image.data.as_ref().map(|data| data.len()),
            Some((FACE * FACE * 6 * 4) as usize),
            "one RGBA texel per face texel"
        );
    }
}
