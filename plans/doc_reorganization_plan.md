# Documentation Reorganization Plan

Based on findings from `audit/doc_organization_audit.md` (2026-03-13).

Goal: A documentation system where a cold-start LLM reads 2-3 files and has a complete mental model, every document has exactly one job, no fact is stated in more than one place, and critical knowledge that currently lives only in session memory is in proper documentation.

---

## Phase 1: Consolidate the Three Conventions Docs into One

**What:** Merge NORNIR_NAMING.md, NORNIR_ORGANIZATION.md, and NORNIR_BUILDING_AND_COMPOSITION.md into a single document: `NORNIR_CONVENTIONS.md`.

**Why:** These three documents have massive overlap. The binary structure template, writer pattern, hook pattern, crate inventories, tier rules, and dependency rules all appear in at least two of the three. A cold-start session reads all three and encounters the same facts multiple times, wasting context window and creating confusion about which document is authoritative for which fact. NORNIR_ORGANIZATION.md alone is 365 lines and tries to do 5 jobs. The merger eliminates ~40% of total content through deduplication.

**Structure of NORNIR_CONVENTIONS.md:**

```
1. What Nornir Is (3 sentences, from ORGANIZATION)
2. Architecture (three-tier diagram, tier rules — ONE copy)
3. Naming
   - Verb prefix table (ONE copy)
   - Binary name structure
   - Crate naming (_core suffix, capability names)
   - Schema file names
   - What never appears in names
4. Directory Map (from ORGANIZATION, without hardcoded counts)
5. Building
   - Binary structure template (ONE copy)
   - Composition patterns (writers, hooks, senders, QA reports)
   - Dependency lookup table ("I need X, use Y")
   - Anti-patterns (process::exit in helpers, reimplementation, monolith)
6. Deploying
   - Deploy script table (ONE copy)
   - Adding a new crate (step-by-step with Cargo.toml templates)
7. Testing
   - Pure logic testability
   - Security hook dual-direction testing
8. Glossary (common terms, from ORGANIZATION)
```

**Risk:** This is a large mechanical change. All references to the three source documents must be updated. CLAUDE.md, MANDATORY_READ_BEFORE_CODING.md, AUDIT_GUIDE.md, and MEMORY.md all reference the three files by name.

**Dependencies:** None. This is the foundation for all subsequent steps.

**Verification:**
- Every fact in the three source documents appears exactly once in the merged document.
- Every cross-reference to the three source documents is updated.
- `cargo test` still passes (no code changes).
- The merged document is under 300 lines (the three originals total ~620 lines; deduplication should cut to ~350-380, then trim filler to reach ~300).

---

## Phase 2: Fix CLAUDE.md Navigation

**What:** Update CLAUDE.md to be a complete routing table. Add references to all documents a session might need, organized by task.

**Why:** CLAUDE.md is auto-loaded into every session. It is the only guaranteed entry point. Currently it routes to MANDATORY and two of three conventions docs, but says nothing about QUICKSTART, AUDIT_GUIDE, HOOK_DESIGN, or SYN_DESIGN. Sessions that need those documents have no way to discover them.

**New CLAUDE.md structure:**

```
# Nornir -- Session Instructions

## Before Writing Code
Read MANDATORY_READ_BEFORE_CODING.md, then NORNIR_CONVENTIONS.md.

## Deploying
(deploy script table -- this is the ONE authoritative copy that other docs reference by pointing here)

## Key Rules
(unchanged -- 5 bullet points)

## Subsystem Documentation
- Hook system: hooks/HOOK_DESIGN.md
- Quality pipeline (syn): cli/syn_cli/SYN_DESIGN.md
- Audit methodology: audit/AUDIT_GUIDE.md
```

**Risk:** Low. CLAUDE.md is small and the changes are additive.

**Dependencies:** Phase 1 (so we can reference NORNIR_CONVENTIONS.md instead of three files).

---

## Phase 3: Eliminate QUICKSTART.md

**What:** Delete QUICKSTART.md. Move any unique content (CLI usage examples) into NORNIR_CONVENTIONS.md or a subsection of CLAUDE.md.

**Why:** The audit found that QUICKSTART.md contains almost nothing unique. Its "What Nornir Does" section duplicates NORNIR_ORGANIZATION.md. Its architecture section is copy #3 of the tier diagram. Its deploy script list is copy #4. Its "Key Design Principles" restates NORNIR_BUILDING_AND_COMPOSITION.md. The only unique content is CLI usage examples (`saga /path/to/project`, `syn --tool all`, `send_alert --source saga`). These examples are useful but do not justify a standalone document.

**Where to move CLI examples:**
- Quality pipeline examples (saga, syn) belong in cli/syn_cli/SYN_DESIGN.md.
- Hook examples belong in hooks/HOOK_DESIGN.md.
- Sender/writer/check examples can go in a brief "Usage Examples" section at the end of NORNIR_CONVENTIONS.md, or simply be omitted (the --help output of each binary serves this purpose).

**Risk:** Low. No other document references QUICKSTART.md (only NORNIR_ORGANIZATION.md's directory map mentions it, and that map will be updated in Phase 1).

**Dependencies:** Phase 1.

---

## Phase 4: Move AUDIT_GUIDE.md to audit/

**What:** Move AUDIT_GUIDE.md from the root to `audit/AUDIT_GUIDE.md`.

**Why:** The audit guide is not part of the standard reading path for a coding session. It is a specialized document for auditing sessions. Keeping it at the root adds clutter (7 root .md files becomes 6, then 5 after QUICKSTART removal). The `audit/` directory already contains audit reports; the methodology guide belongs with them.

**Risk:** Very low. Update the reference in CLAUDE.md (Phase 2) and any other cross-references.

**Dependencies:** Phase 2 (so the new CLAUDE.md routing table points to `audit/AUDIT_GUIDE.md`).

---

## Phase 5: Slim Down MANDATORY_READ_BEFORE_CODING.md

**What:** Remove the deploy script list and the pre-coding checklist from MANDATORY, since both are defined authoritatively in NORNIR_CONVENTIONS.md. Keep the compliance declaration, the philosophical framing, and the "when unclear" guidance. Remove the duplicated "What Goes Wrong" ending (the document opens and closes with the same story).

**Why:** MANDATORY's purpose is to be a compliance gate -- it establishes the stakes and forces acknowledgment. It should not also be a reference manual. The deploy script list, checklist, and rule summaries all exist (after Phase 1) in NORNIR_CONVENTIONS.md. Having them in MANDATORY means two copies, and MANDATORY's copies are inevitably less detailed than the source.

**Target structure:**

```
1. Opening warning (keep as-is, lines 1-8)
2. The Rule (keep as-is, lines 9-13)
3. Required Reading (update to reference NORNIR_CONVENTIONS.md, 3 lines)
4. Compliance Declaration (keep as-is but update file name reference)
5. When Conventions Are Unclear (keep as-is, lines 100-108)
6. Existing Violations (keep as-is, lines 109-116)
7. The Fundamental Truth (keep as-is, lines 118-127)
```

Remove: deploy script list (lines 48-71), pre-coding checklist (lines 75-88), "What Goes Wrong Without This" section (lines 89-98, already said in the opening).

**Risk:** Low. The removed content is pure duplication.

**Dependencies:** Phase 1.

---

## Phase 6: Relocate Behavioral Norms from MEMORY.md to Workspace Documentation

**What:** Move the "Gleipnir Hook Violations -- Ownership Rule" and "Gleipnir Checks Are Not Surface-Level" sections from MEMORY.md into `audit/AUDIT_GUIDE.md` (or the top of MANDATORY_READ_BEFORE_CODING.md).

**Why:** These are critical behavioral corrections that prevent specific LLM failure modes. They currently live only in MEMORY.md, which is an auto-memory file that can be reset, is specific to one user's Claude Code configuration, and is not version-controlled with the workspace. If MEMORY.md is lost, these corrections vanish. They belong in a workspace document that is checked into git.

**Best location:** `audit/AUDIT_GUIDE.md`, as a new section "LLM Behavioral Norms" between the introduction and Priority 1. The audit guide already addresses how LLMs should think about code quality; these rules extend that to how LLMs should think about their own violations.

**Risk:** Very low. The content already exists; this is relocation.

**Dependencies:** Phase 4 (so we know the audit guide's location).

---

## Phase 7: Clean Up MEMORY.md

**What:** Remove from MEMORY.md all content that is now in workspace documentation. This includes:
- Conventions section (duplicates NORNIR_CONVENTIONS.md)
- Workspace dependencies list (duplicates NORNIR_CONVENTIONS.md)
- Behavioral norms (moved to audit guide in Phase 6)
- Current Improvement Plan section (all items DONE)
- Dead link to architecture.md
- Stale "Current State" snapshot

Keep:
- Project Identity (1 line summary)
- Test Command (not documented elsewhere)
- Completed Work (historical, useful for context)
- Audit Reports pointer (still valid)

**Why:** MEMORY.md should contain only knowledge that is genuinely session-specific or operational (test commands, completed work log). Architectural rules and conventions should live in the workspace, not in transient memory.

**Risk:** Low. The removed content exists in workspace docs.

**Dependencies:** Phases 1 and 6.

---

## Phase 8: Clean Up Subsystem Design Docs

**What:** For both hooks/HOOK_DESIGN.md and cli/syn_cli/SYN_DESIGN.md:
1. Remove completed migration history (HOOK_DESIGN "Rename Migration -- COMPLETE" section).
2. Remove implementation status sections (SYN_DESIGN "Current State" section) or update them.
3. Remove dependency inventories (SYN_DESIGN "Dependencies / Already Built") -- these belong in NORNIR_CONVENTIONS.md's crate inventory.
4. Remove duplicated pipeline descriptions -- reference HOOK_DESIGN.md from SYN_DESIGN.md instead of repeating the saga-syn flow.

**Why:** Design docs should describe design, not status. Status information goes stale immediately. Dependency inventories drift from reality. The pipeline description exists in three places; it should be in one.

**Risk:** Low. These are documentation-only changes.

**Dependencies:** Phases 1-2 (so references can be updated).

---

## Phase 9: Create Next Improvement Plan from Untracked Audit Findings

**What:** Create `plans/IMPROVEMENT_PLAN_2.md` (or rename the current one to `plans/improvement_plan_2025_03_12.md` and create a new `plans/IMPROVEMENT_PLAN.md`) from the unaddressed findings in strict_audit_A.md and strict_audit_B.md.

Known unaddressed findings:
- P1-01 (Audit B): Pure logic in traffic_interceptor_rewriter
- P1-02 (Audit B): Pure logic in watch_and_diff_exchange_intercepts
- P2-01 (Audit B): datagram_types naming (missing _core suffix)
- P3-01 (Audit B): schema_core .expect() calls
- P4-01 (Audit A): io_check/io_filter gate response duplication
- P4-01 (Audit B): traffic_interceptor_rewriter append_raw missing fsync
- P4-02 (Audit B): watcher transcript append missing fsync
- P7-01 (Audit B): syn config lacks schema
- P8-01 (Audit A): io_check/io_filter naming (io_ prefix vs _io suffix)
- P10-01 (Audit B): io_filter orphaned crate

**Why:** The current IMPROVEMENT_PLAN.md is complete. New audit findings have no tracking document. Without a plan, the next session has no roadmap and may re-audit or miss known issues.

**Risk:** Low.

**Dependencies:** None (can be done at any phase).

---

## Phase 10: Remove Stale References

**What:** Fix all stale references found during the audit:
1. NORNIR_ORGANIZATION.md directory map: remove PLAN.md reference (file deleted).
2. MEMORY.md: remove `[architecture.md](architecture.md)` link (file does not exist).
3. NORNIR_ORGANIZATION.md: remove hardcoded crate counts ("8 CLI validation binaries", "33 PyO3 gate modules", etc.) or replace with "see directory map below."
4. QUICKSTART.md: same hardcoded counts (moot if QUICKSTART is deleted in Phase 3).

**Why:** Stale references erode trust in documentation. An LLM that follows a link to a nonexistent file loses confidence in the documentation system.

**Risk:** Very low.

**Dependencies:** Can be done at any phase. Some are subsumed by earlier phases (e.g., QUICKSTART deletion in Phase 3 eliminates its stale counts).

---

## Execution Order

The phases are numbered in dependency order:

```
Phase 1  (consolidate three docs into one) -- foundation
Phase 2  (fix CLAUDE.md navigation) -- depends on 1
Phase 3  (eliminate QUICKSTART.md) -- depends on 1
Phase 4  (move AUDIT_GUIDE.md to audit/) -- depends on 2
Phase 5  (slim MANDATORY) -- depends on 1
Phase 6  (relocate behavioral norms from MEMORY) -- depends on 4
Phase 7  (clean up MEMORY.md) -- depends on 1, 6
Phase 8  (clean up subsystem design docs) -- depends on 1, 2
Phase 9  (create next improvement plan) -- independent
Phase 10 (remove stale references) -- independent, partially subsumed by 1-8
```

Phases 9 and 10 can be done at any time. Phases 1-8 should be done in order.

---

## Expected Outcome

**Before (current state):**
- 7 root .md files + CLAUDE.md
- 14 distinct facts duplicated 2-6 times across documents
- 4 orphaned documents (QUICKSTART, AUDIT_GUIDE, HOOK_DESIGN, SYN_DESIGN)
- 2 dead references (PLAN.md, architecture.md)
- Critical behavioral norms in transient memory only
- Cold-start reading path: 4 files, ~1200 lines of content with significant redundancy

**After (target state):**
- 3 root .md files (CLAUDE.md, MANDATORY_READ_BEFORE_CODING.md, NORNIR_CONVENTIONS.md)
- Each fact stated exactly once
- Every document reachable from CLAUDE.md
- Behavioral norms in version-controlled workspace documentation
- Cold-start reading path: 2-3 files, ~500 lines of non-redundant content
- Subsystem docs preserved in their directories, referenced from CLAUDE.md
- Active improvement plan with tracked findings
