# Physics Sandbox — Master Physics Plan

Current state: cellular-automaton sandbox with ~30 materials, heat diffusion,
combustion, melting, explosions, and basic structural collapse.

## Phase B: Velocity & Force (do first)

Per-cell velocity (vx, vy). Gravity accelerates falling grains; buoyancy lifts
gases; drag caps terminal speed; collisions bounce/slide/dampen. Replaces
hard-coded fall_speed_of / spread_of / fling_outward magic numbers with natural
momentum. Everything downstream benefits.

## Phase C: Air / Wind

Per-cell pressure scalar. Heat sources expand air; gases compress; pressure
gradients create wind that pushes loose materials and feeds/fights fire.
Steam explosions, smoke columns, fire drafts.

## Phase A: Explosion Perfection

Shockwave propagation over multiple ticks (not instant). Chain reactions.
Structural-component push from blast pressure. Shaped charges. All built on
top of velocity + pressure.

Build order regardless of the letter labels: **B (velocity) → C (air/pressure)
→ A (explosions)** — each phase depends on the one before it.

---

Each phase should be:
- Simple: one clear concept, not a research paper
- Fast: no per-tick allocations, O(cells) with small constants
- Realistic: emergent behavior, not scripted
- Discrete: cell-grid native, no floating-point coordinates
