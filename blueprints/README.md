# Agent map blueprints

Create a JSON blueprint, validate it, then build it into a scene loadable from the in-game scene menu:

```sh
cargo run -- --map-validate blueprints/skyscraper.json
cargo run -- --map-build blueprints/skyscraper.json
```

Coordinates start at the top-left. Supported materials are `empty`, `stone`, `concrete`, `metal`, `glass`, `wood`, `sand`, `coal`, `water`, `oil`, `napalm`, `liquid_nitrogen`, `acid`, `lava`, `fire`, `steam`, `ember`, `ash`, `smoke`, `salt`, `ice`, `gunpowder`, `fuse`, `tnt`, `c4`, `plant`, and `mercury`.

Supported operations:

- `rect`: `x`, `y`, `width`, `height`, `material`
- `hollow_rect`: rectangle fields plus optional `thickness`
- `line`: `from`, `to`, optional `thickness`, `material`
- `disc`: `center`, `radius`, `material`
- `ground`: optional `thickness`, `material`
- `building`: `x`, `y`, `width`, `height`, optional `wall_thickness`, optional `floors`, `material`
- `stairs`: `x`, `y`, `steps`, optional `step_width`, optional `rise_left`, `material`
- `tank`: `x`, `y`, `width`, `height`, optional `thickness`, `wall`, `contents`, optional `fill` from 0 to 1
- `repeat`: `count`, `[x,y]` `offset`, and nested `operations`
- `prefab`: `name`, `x`, `y`; inserts an operation list declared in the top-level `prefabs` object
- `terrain`: two or more `[x,y]` `points`, positive `depth`, `material`; interpolates a surface and fills downward
- `vary`: rectangle fields, `from`, non-empty `materials`, optional `chance` from 0 to 1, optional integer `seed`; deterministically textures matching cells

Example prefab declaration and placement:

```json
{
  "prefabs": {
    "window": [{"op":"rect", "x":0, "y":0, "width":6, "height":3, "material":"glass"}]
  },
  "operations": [
    {"op":"prefab", "name":"window", "x":20, "y":12}
  ]
}
```

Prefab coordinates are local to each placement. Prefabs may use other prefabs, but cyclic or unknown references are rejected. `vary` only replaces cells currently equal to `from`; using the same seed and geometry always produces the same result.

Operations are applied in order, so later operations can add doors, windows, contents, and damage by painting over earlier ones. Shapes outside the map are clipped with a warning.
