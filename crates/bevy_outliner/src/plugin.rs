use bevy::{
    asset::embedded_asset,
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    prelude::*,
    render::{
        extract_component::ExtractComponentPlugin,
        render_graph::{RenderGraphApp, ViewNodeRunner},
        render_resource::SpecializedRenderPipelines,
        Render, RenderApp, RenderSet,
    },
};

use crate::{
    components::{Outline, OutlineCamera, OutlineSettings},
    node::{
        prepare_outline_bind_groups, prepare_outline_textures, OutlineNode, OutlineNodeLabel,
    },
    pipeline::OutlinePipelines,
    silhouette::{
        extract_outline_meshes, prepare_silhouette_bind_groups, queue_outline_silhouettes,
        SilhouetteNode, SilhouetteNodeLabel, SilhouettePipeline,
    },
};

/// Plugin that enables JFA-based object outlining.
///
/// Add this plugin to your app, then add [`Outline`] components to entities
/// you want outlined, and [`OutlineCamera`] to cameras that should render outlines.
pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        // Embed shaders
        embedded_asset!(app, "shaders/jfa_init.wgsl");
        embedded_asset!(app, "shaders/jfa_flood.wgsl");
        embedded_asset!(app, "shaders/outline_composite.wgsl");
        embedded_asset!(app, "shaders/outline_silhouette.wgsl");

        app.init_resource::<OutlineSettings>()
            .add_plugins((
                ExtractComponentPlugin::<Outline>::default(),
                ExtractComponentPlugin::<OutlineCamera>::default(),
            ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<OutlinePipelines>()
            .init_resource::<SilhouettePipeline>()
            .init_resource::<SpecializedRenderPipelines<SilhouettePipeline>>()
            .add_systems(ExtractSchedule, extract_outline_meshes)
            .add_systems(
                Render,
                (
                    prepare_outline_textures.in_set(RenderSet::PrepareResources),
                    queue_outline_silhouettes.in_set(RenderSet::Queue),
                    (prepare_outline_bind_groups, prepare_silhouette_bind_groups)
                        .in_set(RenderSet::PrepareBindGroups),
                ),
            )
            .add_render_graph_node::<ViewNodeRunner<SilhouetteNode>>(Core3d, SilhouetteNodeLabel)
            .add_render_graph_node::<ViewNodeRunner<OutlineNode>>(Core3d, OutlineNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::EndMainPass,
                    SilhouetteNodeLabel,
                    OutlineNodeLabel,
                    Node3d::Tonemapping,
                ),
            );
    }
}
