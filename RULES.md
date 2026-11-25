# `rotor_tree` Generation Rules

Simple summary of every rule currently enforced by `targets/examples/rotor_tree.rs` (one rule per sentence).

## Lineage & Slots
- The grammar picks the number of lineage blocks (bounded by `--depth`) and 0 to 4 forks for each lineage slot.
- Each lineage block is labeled `<slot>a` and builds on the previous lineage block, or genesis `0` for the first.
- Slots between consecutive lineage blocks are randomly skipped (`1/4` chance per step) while keeping every gap strictly smaller than `--slot-max`.
- Every lineage block is finalized except the tip, which is left as an unfinalized open tip in `1/3` of scenarios.

## Forks
- Each lineage slot gets 0 to 4 forks labeled `<slot>b` onward, and each skipped slot gets 0 to 5 forks (weighted: 0 a third of the time, 1 two-ninths, and 2–5 one-ninth each).
- Half the time a fork builds on the previous lineage block, and the other half it builds on any earlier lineage block or fork.
- Forks are never finalized.

## Dead Blocks
- Each non-finalized block (any fork or an open tip) is marked `dead` with `1/4` probability, meaning its footer declares a wrong bank hash.
- Finalized lineage blocks are never dead.

## Fast Leader Handover (FLH)
- Only blocks at a leader window start (`slot > 0` and `slot % 4 == 0`) can use FLH.
- With `1/2` probability, an eligible window-start block picks an `optimistic_parent` from existing blocks satisfying `parent_slot <= opt_slot < slot` and `opt != parent`.

## Certificates (`NOTAR`, `NOTAR_FALLBACK`, `FINAL`, `SKIPPED`)
- Every finalized lineage block gets `FINAL` unless randomly omitted (`1/4` chance for implied finalization), except the last finalized block and any block needed to keep the next slot inside `--slot-max` always get `FINAL`.
- Every gap slot before a finalized block receives a `SKIPPED` certificate.
- A directly finalized slot (`FINAL`) has `NOTAR` on its lineage block and zero conflicting `NOTAR_FALLBACK` certificates.
- An implied-finalized lineage block (finalized without `FINAL`) keeps `NOTAR` half the time and switches to `NOTAR_FALLBACK` the other half.
- An eligible alive open tip gets `NOTAR` (`1/4`), `NOTAR_FALLBACK` (`1/4`), or no certificate (`1/2`).
- Each eligible non-dead fork in a slot without `FINAL` gets `NOTAR_FALLBACK` with `1/2` probability, capped at 1 conflicting `NOTAR_FALLBACK` alongside a `NOTAR` or 4 total `NOTAR_FALLBACK` blocks without one.
- A block is only eligible for a certificate if it is not dead, its parent is genesis or certified, and either `slot == parent_slot + 1` or `slot` is a window start (`1` or `slot % 4 == 0`) with every gap slot in between `SKIPPED`.

## Repair
- Every finalized lineage block is repairable (`REPAIR`).
- An open tip is repairable with `1/2` probability, and a skipped slot with forks picks one random fork to be repairable with `1/2` probability.
- At most one version per slot is repairable, and a fork sharing a slot with a lineage block is never repairable.

## Action Delivery & Ordering
- Every block emits `TURBINE`, plus `REPAIR`, `NOTAR`, `NOTAR_FALLBACK`, and `FINAL` according to its flags, and every skipped slot emits `SKIPPED`.
- All actions are shuffled and released once they fall within `--slot-max` slots of the deepest finalized block whose lineage prefix has been delivered.
- A fork's `TURBINE` waits for its slot's lineage block's `TURBINE` whenever that lineage block is not repairable.
- A lineage block's `FINAL`, `NOTAR`, or `NOTAR_FALLBACK` waits until that block and every earlier lineage block has been delivered by `TURBINE` or is repairable.
- A `SKIPPED` action waits until a `FINAL` for a later slot has been emitted.

## Replay Expectations
- A block gets `replay: "yes"` only if it is a lineage block with no dead ancestors that is either finalized or un-forked and repairable.
- Every fork, every descendant of a dead block, and every unfinalized lineage block without guaranteed delivery gets `replay: "maybe"`.
