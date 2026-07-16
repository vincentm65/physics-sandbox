# Physics Rewrite Plan

**Build order: B → C → A.** Velocity is the shared foundation; atmosphere depends
on movement; explosions depend on both.

## Phase B — Velocity and Movement [Complete]

Persistent bounded velocity drives movable materials without tunneling. Vertical
velocity and displacement use quarter-cell fixed point so gravity and buoyancy
accelerate gradually.

- Gravity for powders, liquids, embers, and sparks.
- Buoyancy for fire, smoke, and steam.
- Bounded DDA traversal through every crossed cell.
- Direction-aware displacement and collision reset.
- Lifecycle and reactions run before movement.
- Scene, undo, resize, structural movement, and explosion impulses preserve
  velocity state.

**Status:** complete. Low-speed liquid leveling is hydrostatic/velocity-driven;
multi-cell flow teleports have been removed.

## Phase C — Full-Resolution Atmosphere [Complete]

Atmosphere uses arrays aligned one-to-one with material cells. This preserves
single-cell walls, holes, leaks, vents, and containers without coarse-grid
connectivity reconstruction. Empty cells and visible gases are air-permeable;
solids and liquids displace air.

### C0 — Symmetric fixed-point movement [Complete]

- Add fractional horizontal velocity and displacement (`vx_frac`, `x_frac`).
- Apply pressure as small impulses on either axis.
- Preserve both axes through movement, collisions, saves, undo, resize,
  structural translation, and explosion flings.

### C1 — Conserved air and pressure [Complete]

- Store fixed-point air mass per material cell; derive pressure from mass and
  local temperature.
- Exchange bounded mass through cardinal connections at full resolution.
- Treat world edges as ambient vents and material edits as topology changes.
- Process active chunks and reactivate neighboring chunks when flow crosses a
  boundary; do not scan an equilibrium world unnecessarily.

### C2 — Oxygen and combustion products [Complete]

- Store oxygen and exhaust as conserved portions of each cell's gas mixture.
- Fire consumes local oxygen using an effective 2.5D reserve and produces hot
  exhaust.
- Low oxygen weakens heat and spread while increasing smoke; only sustained
  critical oxygen starvation extinguishes fire.
- Openings replenish oxygen through the same one-cell transport paths used by
  pressure.
- Keep visible smoke as a material initially; airflow transports it while the
  atmospheric fields represent invisible gas composition.

### C3 — Combustible gas [Complete]

- Store fuel vapor as another transported gas component.
- Hot oil and napalm emit vapor into adjacent open cells.
- Fire consumes ignitable oxygen/fuel mixtures and rapid burning adds heat and
  temporary overpressure.
- Keep the species list fixed and compact; add new gases only for demonstrated
  gameplay needs.

### C4 — Coupling, controls, and diagnostics [Complete]

- Pressure gradients apply fixed-point impulses to fire, smoke, steam, loose
  powders, liquids, sparks, and movable debris according to resistance.
- `Air Physics` can be toggled without deleting its state; `Reset Atmosphere`
  explicitly restores ambient air.
- Independent overlays show pressure, oxygen, fuel vapor, exhaust, temperature,
  and airflow without changing simulation.
- Scene files and undo snapshots preserve atmosphere and remain backward
  compatible by initializing missing fields from material permeability.

**Done when:** a one-cell opening vents pressure and gas, smoke follows the draft,
sealed fires consume oxygen and extinguish, ventilated fires continue burning,
fuel vapor leaks and ignites, atmosphere can be toggled or inspected, and the
million-cell equilibrium selftest remains practical.

**Status:** complete. One-cell venting, draft-driven smoke, staged combustion,
ventilation, fuel-vapor ignition, controls, persistence, and diagnostics are
implemented and covered by tests.

## Phase A — Explosions and Structural Impacts [Complete]

Build explosions from velocity and atmospheric pressure instead of directly
relocating cells.

- Blast creates radial pressure and heat.
- Pressure accelerates movable debris.
- Structural components receive impulse and accumulated damage.
- Unsupported or over-damaged structures detach, fall, and collide.
- Different explosives vary by pressure, heat, radius, and duration.
- Remove `fling_outward` and other explosion-specific movement hacks.

**Done when:** blasts propagate through exact openings, throw debris, damage
nearby structures, and behave differently in sealed versus open spaces.

**Status:** complete. Instantaneous blast profiles (gunpowder/TNT/C4) apply
LOS-aware damage, heat, and outward velocity impulses; atmosphere receives a
matching overpressure injection. Structural solids accumulate damage against
per-material HP and break into debris; fluids keep their cells and ride
velocity rather than being relocated by `fling_outward`.

## Shared Rules

- Material velocity travels with material state; atmosphere remains spatial and
  flows through passable cells.
- Newly created material starts with zero velocity unless explicitly impulsed.
- Every material receives lifecycle and reaction processing once per tick.
- Movement and atmospheric transfer are bounded, deterministic, and
  allocation-free during steady-state ticks.
- Each phase preserves scene loading, undo, resizing, chunk activation, and
  deterministic self-tests.
