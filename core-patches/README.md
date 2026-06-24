# core-patches/

**Mirror, not canonical.** The few C seams into the Window Maker core live in
the separate `tacitness/wmaker` repository (rebased on
`repo.or.cz/wmaker-crm`). This directory holds a *read-only reference copy* of
that small, documented, upstream-bound patch series so the runtime contract is
visible alongside the Rust companions.

Do not develop core C here. Edits flow through the C repo and are intended for
upstream via `git send-email`, then deleted from the series once accepted
(PLAN §5, §9). Empty until the first seam (the tiling-mode spike, Week 4).
