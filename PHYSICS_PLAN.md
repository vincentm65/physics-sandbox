# Physics Rewrite Plan

**Build order: B → C → A.** Velocity is the shared foundation; pressure depends
on movement; explosions depend on both.

## Phase B — Velocity and Movement

Add persistent per-cell integer velocity (`vx`, `vy`, capped at ±4).

- Gravity for powders, liquids, embers, and sparks.
- Buoyancy for fire, smoke, and steam.
- Bresenham traversal for fast movement without tunneling.
- Direction-aware displacement: dense materials sink, light materials rise, and
  horizontal movement requires empty space.
- Simple collision responses, drag, liquid leveling, powder avalanche, and gas
  ceiling spread.
- Run lifecycle and reactions before movement every tick.
- Replace `fall_speed_of`, `spread_of`, `flow`, and manual rise/fall logic.

**Done when:** existing materials still react and expire correctly while movement
uses velocity exclusively.

## Phase C — Air and Pressure

Add a low-resolution pressure grid updated independently from material cells.

- Pressure diffuses between neighboring regions.
- Fire and explosions increase local pressure.
- Open space equalizes pressure; sealed rooms retain it.
- Pressure gradients apply velocity to gases, liquids, and loose particles.
- Avoid full fluid simulation, temperature coupling, or per-cell air particles.

**Done when:** smoke follows drafts, openings vent pressure, and enclosed blasts
are stronger than open-air blasts.

## Phase A — Explosions and Structural Impacts

Build explosions from velocity and pressure instead of directly relocating cells.

- Blast creates radial pressure and heat.
- Pressure accelerates movable debris.
- Structural components receive impulse and accumulated damage.
- Unsupported or over-damaged structures detach, fall, and collide.
- Different explosives vary by pressure, heat, radius, and duration.
- Remove `fling_outward` and other explosion-specific movement hacks.

**Done when:** blasts propagate through openings, throw debris, damage nearby
structures, and behave differently in sealed versus open spaces.

## Shared Rules

- Velocity travels with material state.
- Newly created material starts with zero velocity unless explicitly given an
  impulse.
- Every material receives lifecycle and reaction processing once per tick.
- Movement is bounded and allocation-free.
- Each phase preserves scene loading, undo, resizing, chunk activation, and
  deterministic self-tests.
