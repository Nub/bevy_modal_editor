//! Shared utility functions for the editor

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::constants::sizes;

/// Calculate the half-height of an object along a surface normal direction.
/// This determines how far to offset the object from a surface so it sits on top.
///
/// # Arguments
/// * `collider` - Optional collider to get AABB from
/// * `surface_normal` - The normal direction of the surface
///
/// # Returns
/// The half-extent of the collider along the axis most aligned with the surface normal,
/// or `sizes::DEFAULT_HALF_HEIGHT` if no collider is provided.
pub fn get_half_height_along_normal(collider: Option<&Collider>, surface_normal: Vec3) -> f32 {
    let Some(collider) = collider else {
        return sizes::DEFAULT_HALF_HEIGHT;
    };

    get_half_height_along_normal_from_collider(collider, surface_normal)
}

/// Calculate the half-height from a collider along a surface normal direction.
/// Use this when you have a guaranteed collider reference.
pub fn get_half_height_along_normal_from_collider(collider: &Collider, surface_normal: Vec3) -> f32 {
    // Get AABB half-extents (at identity rotation since we want object-space extents)
    let half_extents = collider.aabb(Vec3::ZERO, Quat::IDENTITY).size() * 0.5;

    // Find which axis the surface normal is most aligned with
    let abs_normal = surface_normal.abs();
    if abs_normal.x >= abs_normal.y && abs_normal.x >= abs_normal.z {
        half_extents.x
    } else if abs_normal.y >= abs_normal.x && abs_normal.y >= abs_normal.z {
        half_extents.y
    } else {
        half_extents.z
    }
}

/// Calculate a rotation quaternion that aligns the local Y axis with the given normal
pub fn rotation_from_normal(normal: Vec3) -> Quat {
    let up = Vec3::Y;

    if normal.dot(up).abs() > 0.999 {
        if normal.y > 0.0 {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_x(std::f32::consts::PI)
        }
    } else {
        Quat::from_rotation_arc(up, normal)
    }
}
