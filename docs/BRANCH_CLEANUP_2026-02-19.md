# Branch Cleanup Audit — 2026-02-19

Audit of 13 stale remote feature branches in the PWE repository.
All branches evaluated against `develop` (at `24c14ad`).

---

## Merged to Develop (6 branches)

### 1. `feature/block-content-validation`
- **Last commit:** 2026-02-15 — `feat: add block content validation for hot-reload`
- **Changes:** 3 files, +1048 lines (block_validator.rs, tests)
- **Merge:** Clean auto-merge, no conflicts
- **Rationale:** Complete validation system for BlockDefinition with comprehensive test coverage

### 2. `feature/chunk-mesh-caching`
- **Last commit:** 2026-02-13 — `feat: implement chunk mesh caching to reduce load time`
- **Changes:** 4 files, +564 lines (mesh_cache.rs, config, world/mod)
- **Merge:** Clean auto-merge
- **Rationale:** Complete mesh caching implementation with cache invalidation and tests

### 3. `feature/editor-validation-feedback`
- **Last commit:** 2026-02-15 — `feat: add real-time validation feedback in block editor`
- **Changes:** 6 files, +853 lines (validation_display.rs, content_editor updates, tests)
- **Merge:** Conflict in `block_validator.rs` (both branches created it). Resolved by keeping block-content-validation's comprehensive version and adapting editor code to use its API. Added compatibility aliases.
- **Rationale:** Adds visual validation feedback in the block editor UI using egui

### 4. `feature/block-render-batching`
- **Last commit:** 2026-02-15 — `feat: implement block render batching for draw call reduction`
- **Changes:** 2 files, +601 lines (render_batching.rs)
- **Merge:** Clean auto-merge
- **Rationale:** Complete render batching system with group merging and tests

### 5. `feature/chunk-memory-profiler`
- **Last commit:** 2026-02-13 — `feat(profiler): add per-chunk memory tracking and overlay panel`
- **Changes:** 3 files, +399 lines (chunk_metrics.rs)
- **Merge:** Minor conflict in `world/mod.rs` (resource initialization order). Resolved trivially by including both `.init_resource` lines.
- **Rationale:** Complete memory profiling overlay with per-chunk tracking

### 6. `feature/chunk-preload-hint-system`
- **Last commit:** 2026-02-12 — `feat: add chunk preload hint system for reduced pop-in`
- **Changes:** 2 files, +451 lines (preload_hints.rs)
- **Merge:** Minor conflict in `world/mod.rs` (module declarations). Resolved trivially.
- **Rationale:** Complete preload hint system with priority queue and tests

---

## Already Merged / Closed (3 branches)

### 7. `feature/minimap-biome-colors`
- **Last commit:** 2026-02-08
- **Status:** Already fully merged into develop (branch tip is an ancestor of develop)
- **Action:** Safe to delete remote branch

### 8. `feature/metrics-dashboard-unified`
- **Last commit:** 2026-02-07
- **Status:** Already fully merged into develop (branch tip is an ancestor of develop)
- **Action:** Safe to delete remote branch

### 9. `release/v0.2.0-save-load`
- **Last commit:** 2026-02-17 — `chore: release v0.2.0-save-load`
- **Status:** Release tag branch. Behind develop (missing v0.2.1 unified debug overlay). Contains only 1 unique commit (`chore: release v0.2.0-save-load`) with version bump in Cargo.toml and README additions.
- **Action:** Keep as release history marker. Do NOT delete.

---

## Closed — Superseded (1 branch)

### 10. `feature/resource-monitor-panel`
- **Last commit:** 2026-02-06 — `feat: add resource usage monitor panel to debug console`
- **Changes:** 2 files, +522 lines
- **Status:** Branched from a very old point (before 30+ commits on develop). The resource monitoring functionality has been superseded by the unified debug console and performance overlay (`feat: unified debug console with performance overlay`) merged in v0.2.1.
- **Action:** Safe to delete remote branch

---

## Requires Manual Review (3 branches)

### 11. `feature/chunk-lifecycle-events`
- **Last commit:** 2026-02-14 — `feat: complete chunk lifecycle event system implementation`
- **Changes:** 51 files, +3328/-1726 lines
- **Status:** **8 merge conflicts** with develop (audio/mod.rs, content/block.rs, editor/minimap.rs, generation/mod.rs, main.rs, world/meshing.rs, world/mod.rs, world/texture_atlas.rs). Very large changeset touching many modules.
- **Recommendation:** This branch appears to be a major refactor. Needs dedicated review session to resolve conflicts and verify integration.

### 12. `feature/inventory-health-bridge`
- **Last commit:** 2026-02-16 — `feat: connect inventory and health systems - food consumption + death drops`
- **Changes:** 54 files, +4649/-1720 lines
- **Status:** **10 merge conflicts** including add/add conflicts in health/mod.rs, inventory/mod.rs, inventory_health/mod.rs. Very large changeset.
- **Note:** develop already has `4721bbd feat: connect inventory and health systems` which appears to be the same feature merged via a different path. This branch may be fully duplicate.
- **Recommendation:** Compare branch changes against what's already in develop. Likely fully superseded.

### 13. `feature/shader-hot-reload-caching`
- **Last commit:** 2026-02-13 — `feat(shader-system): add checksum-based caching to hot-reload`
- **Changes:** 8 files, +600 lines
- **Status:** **1 merge conflict** in `src/lib.rs` (module declarations). Should be trivial to resolve but needs verification that shader module integrates cleanly with current codebase.
- **Recommendation:** Low-risk merge candidate. Resolve lib.rs conflict and test.

---

## Branches Deleted from Remote

| Branch | Reason |
|--------|--------|
| `feature/minimap-biome-colors` | Already merged to develop |
| `feature/metrics-dashboard-unified` | Already merged to develop |
| `feature/resource-monitor-panel` | Superseded by unified debug console |
| `feature/block-content-validation` | Merged in this cleanup |
| `feature/chunk-mesh-caching` | Merged in this cleanup |
| `feature/editor-validation-feedback` | Merged in this cleanup |
| `feature/block-render-batching` | Merged in this cleanup |
| `feature/chunk-memory-profiler` | Merged in this cleanup |
| `feature/chunk-preload-hint-system` | Merged in this cleanup |

---

## Summary

| Category | Count |
|----------|-------|
| Merged to develop | 6 |
| Already merged (deleted) | 2 |
| Superseded (deleted) | 1 |
| Requires manual review | 3 |
| Kept (release branch) | 1 |
| **Total evaluated** | **13** |

All tests pass after merges: **805 unit tests + 33 integration tests = 838 total, 0 failures**.
