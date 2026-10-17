# SkelForm File Specification for Runtimes

For the full documentation, please check out
[Dev Docs](https://skelform.org/dev-docs). It can be accessed offline if
SkelForm is installed.

Please ensure you are reading the correct docs version (check `version` in
`armature.json`).

The only files necessary for parsing are `armature.json` and `atlasX.png` (where
X is the atlas number).

## Tips

- All IDs are sequential and start at 0. They may be used directly as array
  indexes

- Bone init fields (`init_*`) should be immutable, and are used to reset their
  respective fields if animation smoothing is enabled

- Animation keyframes always store a single unit of any vector field. Example:
  position is stored as `PositionX` and `PositionY` keyframes, separately

- Tint is stored as a Vector4 (red, green, blue, alpha)

- An animation's `end_frame` is its last frame, where playback loops. When it's 0
  (or missing), the animation ends on its last keyframe. Keys after `end_frame` are
  kept in the file but aren't played

- A mesh's `shapes` (in its visuals) are morph targets: each has a `name`, a setup
  `weight`, and `deltas`, offsets of vertices (`vert` is the vertex index) in the
  mesh's own space. Before skinning, each vertex moves by the sum of
  `weight × delta` over its shapes. `ShapeWeight` keyframes animate a shape's
  weight; their `shape` field is the shape's `id`. Runtimes without shape support
  can ignore them: the mesh then stays at rest
