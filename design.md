# Level editor

The purpose of this project is to create a level editor for bevy games that use the avian3d physics engine. Right now configuring objects and meshes is a lot of manual tedium to apply the correct colliders and collider generators on objects / meshes. I want to utilize bevy + egui to create a level editor that can create a level/scene using primitive shapes + mesh shapes. There should also be the concept of prefab types that can include various other components as a template.

## Tools

- Dependancy management: nix
- Language: Rust

## Features

- Prefab design
    - Collection of 1 or more entities + components on those entities.
    - Must be serialized/deserialized
    - Must be designable in the editor
    - A prefab is essentially a recursion of a level/scene

- Scene/Level
    - This is the root asset that will be loaded into

- The editor should allow the easy addition of components to render gizmos for editing new types of objects

- Object manipulation
    - Rendered gizmos
    - Rotate
    - Scale
    - Move
    - Copy
    - Paste

- Pattern based layouts
    - Repeat linear
    - Repeat on circle

- Modal editing akin to vim
    - View mode
    - Edit mode
        - Rotate,Scale,Move

- Adding/Removing components from objects
    - Editing values of components


