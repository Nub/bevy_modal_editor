# Feature Roadmap

A prioritized list of potential features for bevy_spline_3d, ranked by value.

## Priority Tiers

### Tier 1: High Value / Essential

These features provide the most impact for usability and common use cases.

| Feature | Description | Rationale |
|---------|-------------|-----------|
| **Undo/Redo** | Track changes and allow reverting | Essential for any editor - users won't make changes if they can't undo mistakes |
| **Path Following** | Animate entities along splines | Most common spline use case in games (cameras, enemies, platforms, etc.) |
| **Point Insertion** | Add control point at arbitrary t-value by clicking on curve | Currently can only add at ends - clicking to insert is expected behavior |
| **Axis Constraints** | Lock dragging to X/Y/Z axis | Standard in all 3D tools, critical for precise editing |
| **egui Inspector** | Visual panel for editing spline/road/distribution properties | Makes the tool usable without writing code |

### Tier 2: High Value / Quality of Life

Features that significantly improve the workflow.

| Feature | Description | Rationale |
|---------|-------------|-----------|
| **Multi-Select** | Select multiple points, box selection | Needed for efficient editing of complex splines |
| **Random Variation** | Randomize offset/rotation/scale in distributions | Essential for natural-looking object placement |
| **Cached Gizmo Rendering** | Only resample splines when changed | Performance - enables working with many splines |
| **Snap to Grid** | Configurable grid snapping | Common requirement for level design |
| **Copy/Paste/Duplicate** | Clone splines or selected points | Basic editing operation users expect |

### Tier 3: Medium Value / Extended Functionality

Features that extend capabilities for specific use cases.

| Feature | Description | Rationale |
|---------|-------------|-----------|
| **Variable Width Roads** | Road width changes along spline | Much more realistic roads (intersections, tapers) |
| **Banking/Superelevation** | Tilt road surface on curves | Realistic racing tracks and highways |
| **Custom Cross-Sections** | User-defined extrusion profiles | Enables pipes, rails, fences, walls, etc. |
| **Direction Arrows** | Visual indicators of spline direction | Helps understand flow, especially for paths |
| **Reverse Direction** | Flip spline start/end | Common operation when direction matters |
| **Split/Join Splines** | Divide or merge splines | Needed for complex spline networks |

### Tier 4: Specialized / Nice to Have

Features for advanced or niche use cases.

| Feature | Description | Rationale |
|---------|-------------|-----------|
| **Frenet Frame Visualization** | Show tangent/normal/binormal | Useful for debugging orientation issues |
| **Curvature Combs** | Visualize curvature intensity | Helpful for smooth curve design |
| **Surface Projection** | Project distributions onto terrain | Needed for vegetation/prop placement on terrain |
| **Spline Morphing** | Interpolate between spline shapes | Animation feature for special effects |
| **SVG Import** | Import paths from vector graphics | Useful for 2D-to-3D workflows |
| **LOD System** | Reduce road mesh detail at distance | Optimization for large worlds |
| **Intersections** | Connect multiple roads | Complex feature, high effort |

## Implementation Notes

### Undo/Redo Architecture
```
- Use command pattern: each action creates a reversible Command
- Store command history in a Resource
- Commands: MovePoint, AddPoint, DeletePoint, ChangeSplineType, etc.
- Consider integration with bevy_undo or similar crate
```

### Path Following Component
```rust
#[derive(Component)]
pub struct SplineFollower {
    pub spline: Entity,
    pub speed: f32,           // Units per second
    pub t: f32,               // Current position (0.0 - 1.0)
    pub loop_mode: LoopMode,  // Once, Loop, PingPong
    pub align_to_tangent: bool,
}
```

### Cached Gizmo Approach
```
- Store sampled points in a CachedSplineCurve component
- Only resample when Spline changes (use Changed<Spline> filter)
- Gizmo system reads from cache instead of sampling each frame
```

## Effort Estimates

| Feature | Complexity | Estimated Effort |
|---------|------------|------------------|
| Undo/Redo | High | 2-3 days |
| Path Following | Low | 0.5 days |
| Point Insertion | Medium | 1 day |
| Axis Constraints | Low | 0.5 days |
| egui Inspector | Medium | 1-2 days |
| Multi-Select | Medium | 1 day |
| Random Variation | Low | 0.5 days |
| Cached Gizmos | Low | 0.5 days |
| Variable Width Roads | Medium | 1-2 days |
| Custom Cross-Sections | High | 2-3 days |

## Suggested Implementation Order

1. **Path Following** - Quick win, high value, enables a major use case
2. **Axis Constraints** - Small change, big usability improvement
3. **Point Insertion** - Expected editor behavior
4. **Cached Gizmos** - Performance foundation before adding more features
5. **Random Variation** - Makes distribution plugin much more useful
6. **Undo/Redo** - Complex but essential for serious use
7. **egui Inspector** - Makes tool accessible to non-programmers
8. **Multi-Select** - Builds on existing selection system
