# Phase B: Velocity & Force Physics

## Goal

Replace hard-coded movement tables (`fall_speed_of`, `spread_of`) and ad-hoc
`fling_outward` with persistent per-cell velocity. Gravity accelerates grains;
drag caps terminal speed; collisions bounce and slide. Movement becomes emergent
instead of scripted.

## Design principle: "one clear number per cell"

Each cell gets two new `i8` fields: `vx` and `vy` (cells per tick, range ±4).
No sub-cell positions. No floating point. No mass or torque.

At ~30 ticks/sec, 4 cells/tick = 120 cells/sec — faster than anything on a
terminal-sized grid.

## Data model

```rust
// Added to World:
vx: Vec<i8>,   // horizontal velocity, cells/tick
vy: Vec<i8>,   // vertical velocity, cells/tick (positive = down)
```

Memory: 2 bytes per cell. At 200×100 = 20k cells, that's 40 KB.

Velocity is zero-initialized. Empty cells and static solids keep vx=vy=0.

## Forces (applied each tick before movement)

### Gravity
For powders, liquids, embers, sparks — anything that falls:
```
vy = vy.saturating_add(1)   // accelerate downward 1 cell/tick²
```

### Buoyancy
For gases (Fire, Steam, Smoke, FireworkSpark):
```
vy = vy.saturating_sub(1)   // accelerate upward 1 cell/tick²
```

### Terminal velocity (drag cap)

After gravity/buoyancy, clamp to the material's terminal speed:

| Material | Terminal vy | Notes |
|----------|------------|-------|
| Mercury | +4 | Densest fluid |
| Sand, Salt, Gunpowder, BrokenGlass, Coal | +3 | Dense granular |
| Water, Oil, Acid, LiquidNitrogen | +3 | Low-viscosity liquids |
| Ash | +2 | Light powder |
| Lava, Napalm | +1 | Viscous, sticky |
| Ember | +2 | Slow-burning |
| FireworkSpark | +2 | Light particle |
| Steam, Smoke | −2 | Buoyant gas |
| Fire | −1 | Slow-rising flame |
| Everything else | 0 | Static solids, tools |

### Horizontal drag

Each tick, horizontal velocity decays by one unit toward zero:
```
if vx > 0 { vx -= 1 } else if vx < 0 { vx += 1 }
```

Linear decrement (not multiplicative) is deliberate. A multiplicative decay
like `(vx * 3) / 4` truncates to zero in a single tick for `|vx| == 1`, which
would instantly kill the unit impulses that leveling produces before they ever
take effect. Decrement-by-one preserves them for exactly one tick of movement.

Effect over time: a blast impulse of `vx = 4` glides 4 + 3 + 2 + 1 = 10 cells
across four ticks before exhausting; a leveling impulse of `vx = 1` moves one
cell then is spent, which is the right cadence for a calm surface creeping
toward level.

### No per-material horizontal drag

The unit decay is universal. The terminal-velocity cap already differentiates
materials vertically. Adding per-material horizontal drag would be complexity
without visible benefit.

## Movement

For each cell with non-zero (vx, vy), attempt to move along the velocity vector.

### Decomposition

Move in three phases: diagonal first, then vertical, then horizontal.
Movement uses **local remaining-step counters** derived from `|vx|`/`|vy|`; it
does not modify the stored velocity except on collision. The stored values
persist between ticks and are changed only by forces (gravity, buoyancy, drag,
clamp) and by collision responses.

```
let (svx, svy) = (sign(vx), sign(vy));
let (mut rem_vx, mut rem_vy) = (vx.abs(), vy.abs());

// 0. Diagonal: burn one unit of each axis together for a smooth path.
if vx != 0 && vy != 0 && move_one_step(x, y, svx, svy, allow_diag) {
    rem_vx -= 1;
    rem_vy -= 1;
}
// 1. Vertical: remaining downward (or upward) steps.
for _ in 0..rem_vy {
    if !move_one_step(x, y, 0, svy, allow_vert) {
        vy = 0;          // landed / hit ceiling
        break;
    }
}
// 2. Horizontal: remaining sideways steps.
for _ in 0..rem_vx {
    if !move_one_step(x, y, svx, 0, allow_horiz) {
        vx = -vx / 2;    // bounce with 50% damping
        break;
    }
}
```

The diagonal phase runs first so momentum-bearing cells (blast debris, flowing
liquid with both components) trace smooth diagonal paths instead of
stair-stepping along the axes. Total displacement is always exactly `(|vx|,
|vy|)` — the diagonal step counts against both budgets. Pure fall (`vx = 0`)
and pure drift (`vy = 0`) skip it at zero cost.

For upward movement (gas rising, `vy < 0`), hitting a ceiling sets `vy = -vy / 2`.

### Step function

```
fn move_one_step(&mut self, x, y, dx: i32, dy: i32, allow: impl Fn(Material) -> bool) -> bool:
    // target passes `allow` → swap (velocity travels WITH the material)
    // target fails `allow` → return false (blocked)
```

The `allow` predicate is what preserves today's stratification:
- Vertical moves use `m.can_sink_into(other)` — sand sinks through water,
  mercury through oil, a lighter liquid rises through a denser one.
- Diagonal moves use the same `can_sink_into` predicate (a diagonal is a
  vertical move with a sideways component).
- Horizontal moves use `other.is_empty()` — a cell only slides into open space.

### Displacement

When a fast-moving cell lands on a powder/liquid it can sink through (i.e. the
vertical `allow` predicate accepts the target), they swap. The displaced cell
keeps the position; its velocity is zeroed so it does not inherit the impactor's
momentum as a free gift. It gains its own velocity next tick from gravity. This
is "momentum transfer" simple enough to not need actual momentum math.

## Static equilibrium: leveling & avalanche

Velocity alone does not make a resting fluid seek its level or a resting powder
slump to its angle of repose — a cell with vx = vy = 0 on a flat support never
moves. Without an equilibrium rule, water pyramids and sand forms square cliffs.
This is the one place velocity needs a non-force trigger.

### Liquids: pressure-gradient leveling

After forces and velocity movement, for any liquid whose downward move was
blocked this tick, compare the open-column height to the left and right and set
`vx` toward the shallower side:

```
const LEVEL_DEPTH: usize = 6;   // scan cap; matches the old spread_of ceiling

fn liquid_level(x, y):
    if downward move succeeded: return        // still falling, no leveling needed
    below_left  = open_depth(x-1, y+1, LEVEL_DEPTH)
    below_right = open_depth(x+1, y+1, LEVEL_DEPTH)
    if below_left != below_right:
        vx = sign(below_right - below_left)   // push toward shallower side
        // |vx| == 1: calm surface creep, not a surge
```

`open_depth` counts consecutive empty cells straight down from `(x±1, y+1)`,
stopping at the first non-empty cell or `LEVEL_DEPTH` — whichever comes first.
The cap keeps the scan O(1) per cell (a bounded constant, like the old
`spread_of` range of 6), so a full pool levels in O(cells), not
O(cells × height). Without it, a deep pool would scan the entire column under
every surface cell every tick.

The ±1 magnitude keeps pools glassy. A faucet or a blast sets larger `vx`
directly and still produces fast sideways flow; leveling only governs the calm
surface. This replaces `flow_score`/`spread_of` with a single depth comparison.

### Powders: avalanche trigger

Powders do not level (they keep their pile), but a grain on a slope must roll
off. Unlike leveling, the avalanche is **not** a velocity impulse — it is an
immediate diagonal swap, replicating today's `try_fall` diagonal so the
angle-of-repose behavior is preserved cell-for-cell.

```
fn powder_avalanche(x, y):
    if downward move succeeded: return
    // prefer the lower open diagonal; mirrors today's try_fall diagonal pick
    if open(x-1, y+1) and (not open(x+1, y+1) or roll() < 0.5):
        swap(x, y, x-1, y+1)      // immediate move, one cell down-diagonal this tick
    elif open(x+1, y+1):
        swap(x, y, x+1, y+1)
```

Why immediate, not a velocity impulse: an integer `vx = ±1` set this tick would
need to survive into the next tick to pair with gravity's `vy = +1` and form a
diagonal — but horizontal drag decrements it to zero first, and the movement
loop's diagonal phase runs *before* the vertical phase that detects the block.
The result would be a two-tick sideways-then-drop stair-step, visibly slower and
jerkier than today's smooth roll. An immediate swap sidesteps the entire timing
problem and keeps the observable result identical to the current code.

### Where the equilibrium rules run

Both rules run inside `move_by_velocity`, immediately after the vertical phase
reports "blocked below." Leveling sets a `vx` impulse consumed by the horizontal
phase later in the same tick; avalanche performs its own swap directly. Together
with gravity (falling) and explosions (debris), these are the only things that
move a resting cell, so the system stays simple.

## Velocity source: explosions

`fling_outward` is replaced. Instead of manually placing grains at offset
positions, `explode()` sets velocity on cells in the blast radius:

```
for each cell in blast radius:
    dx = cell.x - blast.x
    dy = cell.y - blast.y
    dist2 = dx*dx + dy*dy
    dist  = max(1, isqrt(dist2))                          // integer sqrt, no floats
    power = ((radius - dist) * MAX_BLAST_SPEED) / radius   // 0 at edge .. MAX at center
    vx = (dx * power) / dist                               // unit dir × power
    vy = (dy * power) / dist
    clamp vx, vy to ±4
```

Then the velocity movement system handles the rest next tick. No more special
`fling_outward` with its hard-coded 2-cell tries.

`MAX_BLAST_SPEED`: 4 cells/tick for TNT/C4, 3 for gunpowder.

### Interaction with the terminal-velocity cap

`apply_forces` clamps `vy` to the material's terminal speed and runs *before*
movement. A blast that flings a sand grain to `vy = +4` finds it clamped to +3
the very next tick, while upward-flung debris (`vy = -4`, uncapped for powders)
flies free until gravity reels it in. Net effect: the downward hemisphere of a
blast is shorter-ranged than the upward hemisphere. This is acceptable — real
debris arcs up and out — but it means blast velocity is not perfectly radial. If
symmetric fling is ever needed, exempt freshly blasted cells from the terminal
cap for one tick via a "blast age" counter; not worth it for Phase B.

## What gets removed

- `fall_speed_of()` — replaced by terminal velocity table
- `spread_of()` — liquids flow horizontally via vx momentum, not a fixed spread count
- `fling_outward()` — replaced by blast velocity impulse
- `flow_score()` / `flow()` — replaced by vx-driven horizontal movement
- `try_fall()` — replaced by the generic velocity movement loop

## What stays (genuinely unchanged)

- `try_step()` / `try_into()` — `move_one_step` builds on them; signatures and
  behavior unchanged
- `moved_tick` — still prevents double-processing
- All reaction, combustion, melting, heat diffusion — untouched
- `adj()`, `idx()`, the `n4`/`n8`/`noise`/`chance`/`roll` helpers, and the
  chunk-activation machinery — unchanged

## What stays (but must learn about velocity)

These mutators are NOT removed, but they must touch `vx`/`vy` or movement breaks
silently. The rule: **velocity travels with the material, not the position.**

- `swap(a, b)` — **swap** `vx`/`vy` along with grid/life/seed/temp. Without this
  a falling grain leaves its downward velocity behind in the empty cell it
  vacated; movement breaks immediately.
- `put(i, m, life)` — **zero** `vx`/`vy`. `put` spawns fresh material (reactions,
  explosions placing fire, faucet spawning water, ice melting). The replacement
  must not inherit the old cell's momentum.
- `paint()` / `paint_state()` — **zero** `vx`/`vy` (brush placement, editor paste).
- `new()` / `clear()` — **zero-initialize** (implied by the zero-init contract).
- `resize()` — **zero-init** the new buffers; existing cells keep their velocity
  through the copy loop (same as temp/life today).
- `restore_from()` — calls `clear()`, so covered transitively.

Missing any one of these is the single most common way this refactor breaks.

## Integration

The `step()` loop gets a velocity movement pass before material-specific logic:

```
for each active cell (bottom-to-top):
    if already moved this tick → skip
    if cell has velocity or should gain velocity:
        apply_forces(cell)             // gravity, buoyancy, drag, clamp
        moved = move_by_velocity(cell) // diagonal, then vy, then vx steps
        if moved:
            activate_next(new_x, new_y) // wake the chunk it landed in
            continue                    // don't also run material step
    // existing material step (reactions, combustion, etc.)
    match material { ... }
```

`apply_forces` only touches fluids, powders, and gases. Static solids, tools,
and empty cells are no-ops.

Note: a cell that moves via velocity skips its material step this tick (the
`continue` above), so fast-moving fluid cells skip `react_cell`. Acid, lava, and
water+lava reactions run slightly slower for fast cells — they react on the tick
they come to rest. This is almost always fine; flag it so nobody is surprised
that a waterfall of acid reacts slower than a puddle.

## Migration order

1. Add `vx`, `vy` to `World`, init to zero. Cover **every** mutator — see "What
   stays (but must learn about velocity)" for the complete list: `new`, `clear`,
   `resize`, `restore_from`, `swap`, `put`, `paint`, `paint_state`. Missing one
   breaks movement silently.
2. Add `terminal_vy(material) -> i8`
3. Add `apply_forces(x, y)` — gravity, buoyancy, drag, clamp
4. Add `move_by_velocity(x, y) -> bool` — diagonal then vertical then horizontal,
   plus the leveling/avalanche triggers on "blocked below"
5. Modify `step()` to call the velocity pass; call `activate_next` on every
   velocity move so fast debris wakes inactive chunks
6. Modify `step_powder()` — remove `try_fall` + diagonals; avalanche replaces them
7. Modify `step_liquid()` — remove `try_fall`, `flow`, `spread`; leveling replaces
   them
8. Modify `step_gas()` — remove manual rise steps; buoyancy + velocity handles it
9. Modify `step_fire()` / `step_ember()` — remove their manual `try_step` rise;
   buoyancy (Fire −1, Ember +2 in the table) drives rise, reactions stay
10. Modify `step_firework()` — replace the seeded `try_step` ascent with an upward
    velocity impulse at ignition; replace `firework_burst`'s offset placement of
    sparks with radial velocity impulses (sparks fly out, then fall via gravity)
11. Modify `step_firework_spark()` — remove manual fall `try_step`; gravity +
    terminal vy (table: +2) handles it
12. Modify `explode()` — replace `fling_outward` with the velocity impulse
13. Leave `step_structural_components()` on its current non-velocity path for now
    (Phase A gives boulders velocity later); document the split so the two
    movement systems don't silently collide
14. No blueprint/save migration needed: blueprints are op-recipes
    (`Deserialize` ops), not per-cell snapshots, so `vx`/`vy` need no format bump
15. Remove dead code: `fall_speed_of`, `spread_of`, `flow_score`, `flow`,
    `fling_outward`
16. Tune terminal velocities, drag, leveling depth, and avalanche bias

## What "realistic but simple" means here

- A sand grain dropped from height accelerates over 3 ticks, not instant full speed
- Water poured on a slope flows sideways with momentum, not a fixed spread radius
- A TNT blast flings particles at different speeds based on distance from center
- Smoke rises, hits a ceiling, spreads sideways with momentum, not static flow scoring
- A falling boulder (structural component) could gain velocity and crush what it lands on — later
