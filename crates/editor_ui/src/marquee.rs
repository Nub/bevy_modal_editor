//! The box you drag to select (spec §9 layout throughput): a translucent
//! rectangle with an accent edge, drawn only while a drag is past the
//! threshold. The kernel owns what the box MEANS — this is the part you see.

use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::style;

#[derive(Component)]
pub(crate) struct MarqueeBox;

/// Spawned hidden once, then moved and resized to follow the drag. Rebuilding a
/// node per frame would churn the UI tree for something that changes only in
/// position and size.
pub(crate) fn spawn_marquee(mut commands: Commands) {
    commands.spawn((
        MarqueeBox,
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            top: px(0.0),
            width: px(0.0),
            height: px(0.0),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(style::radius::S)),
            ..default()
        },
        BorderColor::all(style::color::accent()),
        // Barely-there fill: the box has to read as a region without hiding
        // what is inside it, which is the thing you are aiming at.
        BackgroundColor(Color::srgba(0.35, 0.62, 1.0, 0.10)),
        Visibility::Hidden,
        (GlobalZIndex(600), bevy::picking::Pickable::IGNORE),
    ));
}

pub(crate) fn sync_marquee(
    marquee: Res<Marquee>,
    state: Res<EditorState>,
    mut boxes: Query<(&mut Node, &mut Visibility), With<MarqueeBox>>,
) {
    let rect = state.active.then(|| marquee.rect()).flatten();
    for (mut node, mut visibility) in &mut boxes {
        match rect {
            Some(rect) => {
                node.left = px(rect.min.x);
                node.top = px(rect.min.y);
                node.width = px(rect.width());
                node.height = px(rect.height());
                if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited;
                }
            }
            None => {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}
