//! Environment lighting while a PREFAB is open (owner direction).
//!
//! `space e` parks the level and puts the template in a scene of its own — and
//! most prefabs are not lit. A barrel carries no lamp, so editing one meant
//! editing a black shape on a black ground, which is not an editor so much as
//! a guess.
//!
//! So a template scene is lit by the EDITOR, from a small set of named rooms,
//! and the default is one that works. The light is view state in the strictest
//! sense: it carries no `SceneId`, so scene capture cannot see it, it never
//! enters a transaction, and it is torn down the moment the template closes.
//! A prefab must never acquire a light it did not ask for just because someone
//! looked at it.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use editor_prefabs::template_mode::TemplateEdit;

/// The rooms on offer. Each is a whole lighting answer — a sky, a key light and
/// an exposure — rather than a slider nobody wants to tune while modelling.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TemplateEnvironment {
    /// Neutral, directional, and the default: the arrangement a turntable
    /// render uses, because it makes both the shape and the finish readable.
    #[default]
    Studio,
    /// A soft bright dome with no hard key — the light for judging silhouette
    /// and albedo, where a studio key would be telling you about its own
    /// highlight instead.
    Overcast,
    /// Dim and cool, so a lamp, an emissive panel or a glowing rune is the
    /// brightest thing in frame. Judging one of those under a studio key is
    /// judging the key.
    Night,
    /// The prefab's own lights, and nothing else — what the piece will
    /// actually look like in a level that does not light it.
    None,
}

impl TemplateEnvironment {
    pub const ALL: [TemplateEnvironment; 4] = [
        TemplateEnvironment::Studio,
        TemplateEnvironment::Overcast,
        TemplateEnvironment::Night,
        TemplateEnvironment::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TemplateEnvironment::Studio => "studio",
            TemplateEnvironment::Overcast => "overcast",
            TemplateEnvironment::Night => "night",
            TemplateEnvironment::None => "no lighting",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            TemplateEnvironment::Studio => "neutral key and fill — shape and finish",
            TemplateEnvironment::Overcast => "soft dome, no key — silhouette and colour",
            TemplateEnvironment::Night => "dim and cool — for anything that glows",
            TemplateEnvironment::None => "the prefab's own lights only",
        }
    }

    /// How hard the image-based light drives. `None` is the honest zero.
    pub fn intensity(self) -> f32 {
        match self {
            TemplateEnvironment::Studio => 900.0,
            TemplateEnvironment::Overcast => 1_400.0,
            TemplateEnvironment::Night => 120.0,
            TemplateEnvironment::None => 0.0,
        }
    }

    /// The sun over the room, if it has one. Overcast deliberately has none —
    /// that is what makes it overcast.
    pub fn key_light(self) -> Option<(f32, Vec3)> {
        match self {
            TemplateEnvironment::Studio => Some((7_000.0, Vec3::new(-0.45, 0.72, 0.53))),
            TemplateEnvironment::Overcast => None,
            TemplateEnvironment::Night => Some((450.0, Vec3::new(0.35, 0.55, -0.60))),
            TemplateEnvironment::None => None,
        }
    }
}

/// Radiance for the soft dome: sky above, ground bounce below, no sun.
fn overcast_radiance(direction: Vec3) -> [f32; 3] {
    let up = direction.y;
    if up >= 0.0 {
        let t = up.powf(0.8);
        [0.62 + 0.30 * t, 0.66 + 0.30 * t, 0.74 + 0.26 * t]
    } else {
        // Ground bounce is warmer and much weaker — the reason an overcast
        // shot still has a readable underside.
        let t = (-up).powf(0.7);
        [0.46 - 0.20 * t, 0.44 - 0.20 * t, 0.40 - 0.18 * t]
    }
}

/// Radiance for night: cool, dark, and with a horizon so a metal edge still
/// has something to catch.
fn night_radiance(direction: Vec3) -> [f32; 3] {
    let up = direction.y;
    let base = if up >= 0.0 {
        let t = up.powf(0.6);
        [0.05 + 0.07 * t, 0.06 + 0.09 * t, 0.10 + 0.16 * t]
    } else {
        [0.03, 0.03, 0.05]
    };
    let horizon = 1.0 - (up.abs() * 12.0).min(1.0);
    [
        base[0] + 0.05 * horizon,
        base[1] + 0.06 * horizon,
        base[2] + 0.09 * horizon,
    ]
}

/// The built cubemaps, one per room that has one.
#[derive(Resource)]
pub struct TemplateSkies {
    studio: Handle<Image>,
    overcast: Handle<Image>,
    night: Handle<Image>,
}

impl TemplateSkies {
    fn sky(&self, environment: TemplateEnvironment) -> Option<Handle<Image>> {
        match environment {
            TemplateEnvironment::Studio => Some(self.studio.clone()),
            TemplateEnvironment::Overcast => Some(self.overcast.clone()),
            TemplateEnvironment::Night => Some(self.night.clone()),
            TemplateEnvironment::None => None,
        }
    }
}

pub(crate) fn setup_template_skies(
    preview: Res<crate::preview_env::PreviewEnvironment>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    commands.insert_resource(TemplateSkies {
        // The studio room already exists for the previews; one room, one build.
        studio: preview.0.clone(),
        overcast: images.add(crate::preview_env::build_cubemap(overcast_radiance)),
        night: images.add(crate::preview_env::build_cubemap(night_radiance)),
    });
}

/// The editor-owned key light, so it can be found and removed again.
#[derive(Component)]
pub(crate) struct TemplateKeyLight;

/// Put the room up while a template is open, and take it down when it closes.
///
/// Idempotent and change-driven: it writes only when the template state or the
/// chosen room actually moves, because inserting an environment map every frame
/// would re-upload it every frame.
pub(crate) fn sync_template_environment(
    edit: Res<TemplateEdit>,
    chosen: Res<TemplateEnvironment>,
    skies: Option<Res<TemplateSkies>>,
    cameras: Query<
        Entity,
        (
            With<Camera3d>,
            Without<bevy_outliner::prelude::SilhouetteCamera>,
        ),
    >,
    lights: Query<Entity, With<TemplateKeyLight>>,
    mut was: Local<Option<(bool, TemplateEnvironment)>>,
    mut commands: Commands,
) {
    let Some(skies) = skies else { return };
    let now = (edit.active(), *chosen);
    if *was == Some(now) {
        return;
    }
    *was = Some(now);

    for light in &lights {
        commands.entity(light).despawn();
    }
    for camera in &cameras {
        commands
            .entity(camera)
            .remove::<bevy::light::GeneratedEnvironmentMapLight>();
    }
    if !edit.active() {
        return;
    }
    if let Some(sky) = skies.sky(*chosen) {
        for camera in &cameras {
            commands
                .entity(camera)
                .insert(bevy::light::GeneratedEnvironmentMapLight {
                    environment_map: sky.clone(),
                    intensity: chosen.intensity(),
                    ..default()
                });
        }
    }
    if let Some((illuminance, towards)) = chosen.key_light() {
        // No `SceneId`: capture cannot see it, so closing the template can
        // never fold the editor's own light into the prefab.
        commands.spawn((
            TemplateKeyLight,
            DirectionalLight {
                illuminance,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_translation(towards.normalize() * 40.0).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(0),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default has to LIGHT something. A prefab carries no lamp, so a
    /// default of "none" would open every template on a black shape — the bug
    /// this exists for.
    #[test]
    fn the_default_room_is_lit() {
        let chosen = TemplateEnvironment::default();
        assert!(chosen.intensity() > 0.0, "the default room is dark");
        assert!(chosen.key_light().is_some(), "the default room has no key");
    }

    /// Every room is offered, and only one of them is "off".
    #[test]
    fn exactly_one_room_is_unlit() {
        let unlit = TemplateEnvironment::ALL
            .iter()
            .filter(|room| room.intensity() == 0.0)
            .count();
        assert_eq!(unlit, 1);
        assert_eq!(TemplateEnvironment::None.intensity(), 0.0);
    }

    /// Overcast is the one WITHOUT a key — that is what makes it overcast, and
    /// what makes it the right room for judging silhouette.
    #[test]
    fn overcast_has_no_key_light() {
        assert!(TemplateEnvironment::Overcast.key_light().is_none());
        assert!(TemplateEnvironment::Overcast.intensity() > 0.0);
    }

    /// Night is dimmer than studio, or it is not night and nothing that glows
    /// will read against it.
    #[test]
    fn night_is_dimmer_than_studio() {
        assert!(
            TemplateEnvironment::Night.intensity() < TemplateEnvironment::Studio.intensity(),
            "night out-lit the studio"
        );
    }

    /// The dome is brighter above than below: a ground bounce that out-lit the
    /// sky would read as an object floating over a lightbox.
    #[test]
    fn the_overcast_dome_is_brighter_above_than_below() {
        let up = overcast_radiance(Vec3::Y);
        let down = overcast_radiance(Vec3::NEG_Y);
        assert!(up[2] > down[2], "the sky was darker than the ground");
    }

    /// Night keeps a horizon, or a metal edge has nothing to catch and the
    /// whole point of a dark room is lost.
    #[test]
    fn night_keeps_a_horizon() {
        let horizon = night_radiance(Vec3::X);
        let sky = night_radiance(Vec3::Y);
        assert!(
            horizon[2] > sky[2] * 0.5,
            "the horizon vanished into the sky"
        );
    }
}
