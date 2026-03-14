# Documentation Organization Audit

**Date:** 2026-03-13
**Scope:** Structural quality, discoverability, and LLM-navigability of all markdown files in the nornir workspace
**Method:** Full read of every .md file, cross-reference analysis, redundancy mapping, gap analysis

---

## Inventory

14 markdown files across 5 locations (including 1 external memory file):

**Root (7 files):**
- CLAUDE.md
- MANDATORY_READ_BEFORE_CODING.md
- NORNIR_NAMING.md
- NORNIR_ORGANIZATION.md
- NORNIR_BUILDING_AND_COMPOSITION.md
- QUICKSTART.md
- AUDIT_GUIDE.md

**Subdirectories (4 files):**
- hooks/HOOK_DESIGN.md
- cli/syn_cli/SYN_DESIGN.md
- plans/IMPROVEMENT_PLAN.md
- core/compaction_inject_core/instructions/compaction_summary.md

**Audit outputs (2 files):**
- audit/strict_audit_A.md
- audit/strict_audit_B.md

**External (1 file, auto-loaded):**
- ~/.claude/projects/.../memory/MEMORY.md

**Deleted but referenced:**
- PLAN.md (deleted from disk, still referenced in NORNIR_ORGANIZATION.md directory map)

**Referenced but nonexistent:**
- architecture.md (referenced in MEMORY.md as `[architecture.md](architecture.md)`)

---

## Per-Document Assessment

### 1. CLAUDE.md

**Purpose:** Entry point for every Claude Code session. Auto-loaded by the Claude Code framework before any conversation begins.
**Audience:** Cold-start LLM, every session.
**Word count:** ~250 words (35 lines).

**Strengths:**
- Concise. Does its job as a routing document.
- Clear reading order directive: MANDATORY first, then NAMING, then ORGANIZATION.
- Deploy script list with comments is immediately useful.
- Key rules section covers the highest-damage mistakes.

**Problems:**
1. **Does not mention NORNIR_BUILDING_AND_COMPOSITION.md.** The required reading directive says "read MANDATORY, then NAMING, then ORGANIZATION" but omits the third document that MANDATORY itself requires. A session that reads only what CLAUDE.md says to read will miss composition patterns, testing requirements, and dependency rules.
2. **Does not mention QUICKSTART.md.** A session that needs to understand what nornir produces and how the quality pipeline works has no path to that document from CLAUDE.md.
3. **Does not mention AUDIT_GUIDE.md.** An auditing session has no path from CLAUDE.md to the audit guide.
4. **Does not mention HOOK_DESIGN.md or SYN_DESIGN.md.** These are detailed design documents for the two most complex subsystems, but nothing routes to them.
5. **Deploy script list is duplicated** in MANDATORY_READ_BEFORE_CODING.md (lines 58-69), NORNIR_ORGANIZATION.md (lines 280-292), and QUICKSTART.md (lines 29-43). Four copies of the same list.

**Signal-to-noise:** High signal. Every line earns its place. The problem is what is missing, not what is present.

---

### 2. MANDATORY_READ_BEFORE_CODING.md

**Purpose:** Compliance gate. Forces the LLM to acknowledge rules before writing code.
**Audience:** Any LLM session that will modify code.
**Word count:** ~850 words (127 lines).

**Strengths:**
- Tone is effective. The "this is not hypothetical" framing works.
- Compliance declaration is a concrete forcing function.
- Pre-coding checklist is actionable.
- "What Goes Wrong Without This" section uses concrete narrative to illustrate.

**Problems:**
1. **Deploy script list is copy #2** (lines 58-69). Same list as CLAUDE.md, NORNIR_ORGANIZATION.md, and QUICKSTART.md.
2. **Pre-coding checklist duplicates content from all three NORNIR_*.md files.** Every checklist item is a summary of a rule defined in detail elsewhere. If a rule changes, the checklist may not be updated.
3. **The compliance declaration (lines 29-44) restates rules from NORNIR_*.md files.** "Verb-prefix naming" is defined in NORNIR_NAMING.md. "Three-tier dependency model" is defined in NORNIR_ORGANIZATION.md. "Deploy script integration" is defined in NORNIR_ORGANIZATION.md. The declaration is a summary, and summaries drift.
4. **Sections 9-12 ("Existing Violations", "When Conventions Are Unclear", "The Fundamental Truth") are philosophical framing**, not action items. They are valuable for setting the right mental model, but they are mixed in with the actionable checklist and deploy script reference. An LLM re-reading this mid-session to find a specific rule has to scan past philosophy to find procedure.
5. **"What Goes Wrong Without This" section (lines 89-96) tells the same story as the opening paragraph.** The document opens with "this workspace has been destroyed and rebuilt" and closes with "project abandoned, rebuild from scratch." The point is made twice.

**Signal-to-noise:** Medium. The compliance gate and checklist are high-value. The philosophical framing is valuable on first read but becomes noise on re-reads. The deploy script list is pure redundancy.

---

### 3. NORNIR_NAMING.md

**Purpose:** Defines all naming conventions for the workspace.
**Audience:** Any session creating or renaming crates.
**Word count:** ~1000 words (162 lines).

**Strengths:**
- Comprehensive. Every naming pattern is in one place.
- Tables are well-structured and scannable.
- Examples are concrete and real (not hypothetical).
- "What Never Appears" section is a useful negative constraint.
- Known naming exception (traffic_interceptor_rewriter) is documented.

**Problems:**
1. **Verb prefix table (lines 9-26) is duplicated** in NORNIR_ORGANIZATION.md (Directory Map, lines 108-126), AUDIT_GUIDE.md (Priority 5, line 105), QUICKSTART.md (opening list, lines 9-21), and MEMORY.md (Conventions section). Five copies of the category-to-verb mapping.
2. **"Directory name = package name = binary name" rule** is stated here (line 48), in NORNIR_ORGANIZATION.md (line 129), in MANDATORY_READ_BEFORE_CODING.md (compliance declaration, line 35), and in MEMORY.md. Four copies.
3. **Core crate table (lines 60-73) duplicates NORNIR_ORGANIZATION.md** (lines 75-86). Both list all core crates with their purposes. If a core crate is added or renamed, both must be updated.
4. **Capability crate table (lines 79-92) duplicates NORNIR_ORGANIZATION.md** (lines 88-98). Same issue.
5. **Gate naming convention (lines 96-103) is detailed here** but gate crates are also listed in NORNIR_ORGANIZATION.md. The naming convention (stage_direction) and the crate list serve different purposes but live in different files with no cross-reference.

**Signal-to-noise:** High signal, but the crate inventories create maintenance burden. When the naming doc needs to reference what crates exist, it should point to NORNIR_ORGANIZATION.md, not duplicate the list.

---

### 4. NORNIR_ORGANIZATION.md

**Purpose:** Defines directory structure, dependency tiers, deploy scripts, and procedures for adding new crates.
**Audience:** Any session creating new crates or understanding the workspace structure.
**Word count:** ~2200 words (365 lines).

**Strengths:**
- The three-tier diagram is the clearest single-picture explanation of the architecture.
- Directory map is comprehensive and accurate.
- Adding a new crate step-by-step is genuinely useful.
- Cargo.toml templates are copy-pasteable.
- Common terms glossary is well-maintained.

**Problems:**
1. **This is the largest document and tries to do too many things.** It covers: what nornir is, architecture tiers, directory map, architecture patterns (binary structure, writers, hooks, pure/impure), test coverage, common terms, workspace Cargo.toml structure, deploy scripts, adding new crates, Cargo.toml templates, and deployment targets. That is at least 5 distinct topics.
2. **"Architecture Patterns" section (lines 172-214) duplicates NORNIR_BUILDING_AND_COMPOSITION.md.** The binary structure template (parse_args/run/main) appears in both. The writer composition pattern appears in both. The hook pattern appears in both. The pure/impure separation examples appear in both. NORNIR_ORGANIZATION.md has shorter versions, NORNIR_BUILDING_AND_COMPOSITION.md has longer versions with anti-patterns. The two documents tell the same story at different levels of detail.
3. **Deploy script table (lines 280-292) is copy #3** of the deploy script list.
4. **Directory map (lines 54-167) references PLAN.md, which has been deleted from disk.** Stale reference.
5. **Crate inventory counts in the opening section (lines 9-22)** will go stale when crates are added or removed. "8 CLI validation binaries", "33 PyO3 gate modules", "5 writer binaries" are all hardcoded numbers.
6. **Workspace Cargo.toml section (lines 242-274) duplicates information** from the actual Cargo.toml. When a dependency version changes, this section becomes stale. The MEMORY.md also has a copy of workspace dependency versions.
7. **"Adding a New Crate" section (lines 301-356) overlaps heavily** with NORNIR_BUILDING_AND_COMPOSITION.md (which covers composition patterns, writer creation, hook creation).

**Signal-to-noise:** Medium. The document is a kitchen sink. High-value sections (tier diagram, directory map, glossary) are diluted by duplicated patterns and hardcoded counts.

---

### 5. NORNIR_BUILDING_AND_COMPOSITION.md

**Purpose:** How to build things correctly. Composition patterns, anti-patterns, dependency rules, testing.
**Audience:** Any session writing or modifying code.
**Word count:** ~1800 words (274 lines).

**Strengths:**
- Strong opening: "Compose from existing crates. Do not reimplement."
- Anti-pattern examples (WRONG/RIGHT) are effective.
- "What Goes Wrong Without This" narratives are concrete.
- Dependency lookup table (lines 155-176) is the single most useful reference for "I need to do X, which crate does it?"
- Summary table at the end is a quick-reference.

**Problems:**
1. **Binary structure template (lines 15-31) is the same code** as NORNIR_ORGANIZATION.md (lines 176-188). Two copies of the parse_args/run/main pattern.
2. **Writer composition pattern (lines 66-89) duplicates** NORNIR_ORGANIZATION.md (lines 193-203).
3. **Hook pattern (lines 100-115) duplicates** NORNIR_ORGANIZATION.md (lines 205-206).
4. **Dependency rules section (lines 149-185) partially duplicates** NORNIR_ORGANIZATION.md tier rules and NORNIR_NAMING.md tier descriptions.
5. **"Schema-First for Data Validation" section (lines 187-197)** covers the same ground as AUDIT_GUIDE.md Priority 7. Different audiences (builder vs auditor) but the rules are identical.
6. **Testing requirements section (lines 199-226)** includes security hook testing rules that also appear in AUDIT_GUIDE.md Priority 6.

**Signal-to-noise:** High signal per paragraph, but the document as a whole overlaps significantly with NORNIR_ORGANIZATION.md. The two documents have unclear boundaries -- ORGANIZATION covers "what exists and where", BUILDING covers "how to make new things", but both include composition patterns, code templates, and dependency rules.

---

### 6. QUICKSTART.md

**Purpose:** Feature overview and usage examples.
**Audience:** Unclear. Not referenced from any other document except NORNIR_ORGANIZATION.md's directory map.
**Word count:** ~1200 words (189 lines).

**Strengths:**
- Gives concrete CLI examples for every binary category.
- Useful for understanding what nornir actually produces (the consumer's perspective, not the builder's perspective).

**Problems:**
1. **Audience confusion.** Is this for humans who want to use nornir tools? For LLMs that need to understand what nornir produces? It reads like a user guide, but nornir's users are LLMs and hooks, not humans at a terminal.
2. **Architecture section (lines 49-68) is copy #3** of the three-tier diagram. NORNIR_ORGANIZATION.md and MEMORY.md also have it.
3. **Deploy script list (lines 29-43) is copy #4.**
4. **"What Nornir Does" section (lines 3-21) is nearly identical** to NORNIR_ORGANIZATION.md (lines 1-22). Same structure, same counts, same descriptions.
5. **Never referenced from CLAUDE.md** or MANDATORY_READ_BEFORE_CODING.md. A cold-start session has no route to this document unless it reads the directory map in NORNIR_ORGANIZATION.md and decides to look at it.
6. **Key Design Principles (lines 180-189)** restates principles from NORNIR_BUILDING_AND_COMPOSITION.md and NORNIR_ORGANIZATION.md.

**Signal-to-noise:** Low. Almost everything in this document exists in better form elsewhere. The CLI usage examples are the only unique content, and they serve an audience (human terminal users) that barely exists.

---

### 7. AUDIT_GUIDE.md

**Purpose:** Guide for auditing the workspace. Defines architectural invariants and what to look for.
**Audience:** LLM sessions performing audits.
**Word count:** ~2500 words (263 lines).

**Strengths:**
- Well-structured priority system (P1-P10).
- Each priority explains the invariant, why it matters, what correct looks like, and questions to ask.
- "What Gleipnir Covers" section prevents duplicate effort.
- Reference documents section at the end creates navigation.

**Problems:**
1. **Only referenced from audit reports and IMPROVEMENT_PLAN.md**, not from CLAUDE.md or any other entry-point document. An auditing session must discover this document on its own or be told about it externally.
2. **Priorities 1-5 restate architectural rules from NORNIR_BUILDING_AND_COMPOSITION.md** and NORNIR_ORGANIZATION.md with audit-specific framing. The rules themselves (tier model, process::exit discipline, composition, naming) are defined in the building/organization docs. The audit guide adds "here is how to verify these rules" but also restates the rules themselves. If a rule changes, both the source doc and the audit guide must be updated.
3. **The "Questions to ask" format is effective** but creates a large document. At 2500 words, an auditing session spends significant context window on the guide itself.

**Signal-to-noise:** High for an auditing session. Not relevant for any other session type. The restated rules are necessary context (an auditor needs to know the rule to check it) but create a maintenance burden.

---

### 8. hooks/HOOK_DESIGN.md

**Purpose:** Detailed design of the hook subsystem.
**Audience:** Sessions working on hook binaries or hook_io.
**Word count:** ~1500 words (314 lines).

**Strengths:**
- Comprehensive: naming convention, IO contracts, type signatures, dispatch architecture, build/deploy, wiring.
- Code examples show actual JSON shapes and Rust signatures.
- Pipeline diagram (PostToolUse quality assessment) is the clearest explanation of the saga-syn flow in the workspace.

**Problems:**
1. **Orphaned. No document references it.** CLAUDE.md does not mention it. NORNIR_ORGANIZATION.md does not link to it. MANDATORY_READ_BEFORE_CODING.md does not mention it. A session that needs to modify hooks has no way to discover this document exists unless they browse the hooks/ directory.
2. **"Rename Migration -- COMPLETE" section (lines 248-253)** is historical. The migration is done. This section adds no current value and confuses the document's purpose (is it a design doc or a changelog?).
3. **"Planned hooks" table (lines 42-47)** may be stale. hook_start_session_orient and hook_compact_session_preserve are listed as planned but there is no indication whether they are still planned or abandoned.
4. **Build and Deploy section (lines 204-244)** includes settings.json wiring, which is the only place in the entire documentation system where Claude Code settings.json configuration is documented. This is critical integration information buried in a subsystem design doc.
5. **hook_io capability crate section (lines 114-157)** documents internal API signatures. These drift as the code changes. The code is the source of truth for signatures; documenting them here creates a second copy.

**Signal-to-noise:** Medium. High value for hook work, but hidden and partially stale.

---

### 9. cli/syn_cli/SYN_DESIGN.md

**Purpose:** Detailed design of the syn quality policy gate.
**Audience:** Sessions working on syn_cli or syn_core.
**Word count:** ~1800 words (342 lines).

**Strengths:**
- Clear architecture diagram.
- Filter layer explanation (warn/deny) is well-structured.
- Ratchet concept explained thoroughly.
- CLI interface documented with all flags.
- Integration points section shows how syn fits into the hook pipeline.

**Problems:**
1. **Orphaned. No document references it.** Same discoverability problem as HOOK_DESIGN.md.
2. **"Current State" section (lines 329-341)** contains implementation status ("Phase 1 DONE", "ratchet comparison NOT YET IMPLEMENTED"). This is changelog material that will go stale. It already conflicts with reality if any progress has been made on the ratchet.
3. **The saga-syn pipeline (lines 13-15)** is also documented in QUICKSTART.md (lines 77-99) and HOOK_DESIGN.md (lines 257-287). Three descriptions of the same pipeline.
4. **Dependencies section (lines 316-327)** lists "already built" crates that exist in the workspace. This is inventory information that belongs in NORNIR_ORGANIZATION.md, not a design doc.
5. **Config file documentation (lines 260-268)** is the only place where .syn/warn.toml, .syn/deny.toml, and .syn/ratchet.toml are documented. This is user-facing configuration buried in a developer design doc.

**Signal-to-noise:** Medium. Valuable for syn development, but contains both design (stable) and status (unstable) information mixed together.

---

### 10. plans/IMPROVEMENT_PLAN.md

**Purpose:** Tracks the 10-item improvement plan from the dual-agent audit.
**Audience:** Sessions continuing improvement work.
**Word count:** ~650 words (66 lines).

**Strengths:**
- All 10 items are marked DONE with concise summaries of what was done.
- Links to audit reports.

**Problems:**
1. **All items are DONE.** The document is now historical. It has no current action items.
2. **No indication of what comes next.** The audit reports (strict_audit_A.md, strict_audit_B.md) contain new findings that are not in any plan.

**Signal-to-noise:** Low (now that all items are complete). Historical value only.

---

### 11-12. audit/strict_audit_A.md, audit/strict_audit_B.md

**Purpose:** Detailed audit findings from dual-agent audit.
**Audience:** Sessions working on improvements.
**Word count:** ~2500 words each.

**Strengths:**
- Thorough. Every finding has file paths, line numbers, explanation, and fix.
- "Verified Clean Areas" sections prevent re-auditing.

**Problems:**
1. **No plan references them as source material for remaining work.** The IMPROVEMENT_PLAN.md is complete. The new findings in these audits (P4-01/02 in audit B about missing fsync, P1-01 about pure logic in interceptor, P7-01 about syn config lacking schema) have no tracking document.
2. **Line numbers will become stale** as code is modified. The findings reference specific line numbers that shift with every edit.

**Signal-to-noise:** High for their purpose, but they need a successor plan to be actionable.

---

### 13. core/compaction_inject_core/instructions/compaction_summary.md

**Purpose:** Prompt template for context compaction summaries.
**Audience:** Not human-read. Embedded as instruction text by the compaction_inject_core crate.
**Word count:** ~650 words (71 lines).

**Problems:** None from a documentation organization perspective. This is embedded content, not documentation. It does not participate in the documentation system.

---

### 14. MEMORY.md (external, auto-loaded)

**Purpose:** Persistent session memory, auto-loaded into every conversation.
**Audience:** Every Claude Code session.
**Word count:** ~500 words (75 lines).

**Strengths:**
- Contains critical operational knowledge: test command, completed work history, ownership rules.
- "Gleipnir Hook Violations -- Ownership Rule" and "Gleipnir Checks Are Not Surface-Level" are behavioral corrections that prevent specific LLM failure modes.

**Problems:**
1. **References nonexistent architecture.md**: Line 54 contains `See [architecture.md](architecture.md) for full tier breakdown.` This file does not exist.
2. **Duplicates content from root docs.** "Conventions (Critical)" section (lines 57-68) restates rules from NORNIR_NAMING.md, NORNIR_ORGANIZATION.md, and NORNIR_BUILDING_AND_COMPOSITION.md. Workspace dependencies list (lines 71-74) duplicates NORNIR_ORGANIZATION.md (lines 250-268). These copies will drift.
3. **"Current Improvement Plan" section (lines 33-44)** lists items 1-10 but the plan is complete. This section is stale.
4. **"Current State" section says "2026-03-12"** and references "uncommitted changes across ~40+ files." This is a snapshot that becomes stale after every commit.
5. **Contains knowledge that should be in proper documentation.** The "Gleipnir Hook Violations -- Ownership Rule" and "Gleipnir Checks Are Not Surface-Level" sections encode behavioral rules that are not in any workspace document. If MEMORY.md is reset or lost, this knowledge disappears. These should be in AUDIT_GUIDE.md or a dedicated behavioral norms document.

**Signal-to-noise:** Medium. The behavioral corrections are high value but misplaced. The duplicated conventions and stale status information are noise.

---

## System-Level Assessment

### Overall Documentation Architecture

The documentation system has **7 root-level .md files** plus CLAUDE.md, for a total of 8 files a cold-start LLM encounters at the root. This is too many. An LLM cannot read all 8 and retain the nuance of each. The practical reality is that sessions read CLAUDE.md (auto-loaded), MANDATORY (directed by CLAUDE.md), NAMING (directed by MANDATORY), and ORGANIZATION (directed by MANDATORY). That is 4 files. The other 4 root files (BUILDING_AND_COMPOSITION, QUICKSTART, AUDIT_GUIDE, and the deleted PLAN.md) are orphaned from the navigation path -- they exist but nothing routes to them.

### Navigation Flow

The current navigation path is:

```
CLAUDE.md (auto-loaded)
  --> MANDATORY_READ_BEFORE_CODING.md
    --> NORNIR_NAMING.md
    --> NORNIR_ORGANIZATION.md
    --> NORNIR_BUILDING_AND_COMPOSITION.md
```

This path is broken:
1. **CLAUDE.md does not mention BUILDING_AND_COMPOSITION.** It says "read MANDATORY, then NAMING, then ORGANIZATION." MANDATORY mentions all three, but the entry point (CLAUDE.md) skips one.
2. **QUICKSTART.md is unreachable.** Nothing links to it.
3. **AUDIT_GUIDE.md is unreachable** from the normal navigation flow.
4. **HOOK_DESIGN.md and SYN_DESIGN.md are unreachable** from any navigation path.
5. **MEMORY.md references architecture.md which does not exist.**

### Redundancy Map

The following content is duplicated across multiple documents:

| Content | Locations | Copies |
|---------|-----------|--------|
| Deploy script list | CLAUDE.md, MANDATORY, ORGANIZATION, QUICKSTART | 4 |
| Three-tier architecture diagram | ORGANIZATION, QUICKSTART, MEMORY.md | 3 |
| Binary structure template (parse_args/run/main) | ORGANIZATION, BUILDING_AND_COMPOSITION | 2 |
| Writer composition pattern | ORGANIZATION, BUILDING_AND_COMPOSITION | 2 |
| Hook composition pattern | ORGANIZATION, BUILDING_AND_COMPOSITION | 2 |
| Verb prefix table / category mapping | NAMING, ORGANIZATION, AUDIT_GUIDE, QUICKSTART, MEMORY | 5 |
| Core crate inventory | NAMING, ORGANIZATION | 2 |
| Capability crate inventory | NAMING, ORGANIZATION | 2 |
| "What nornir produces" counts | ORGANIZATION, QUICKSTART | 2 |
| Workspace dependency versions | ORGANIZATION, MEMORY | 2 |
| "directory = package = binary" rule | NAMING, ORGANIZATION, MANDATORY, MEMORY | 4 |
| Saga-syn pipeline description | QUICKSTART, HOOK_DESIGN, SYN_DESIGN | 3 |
| process::exit only in main rule | CLAUDE.md, MANDATORY, ORGANIZATION, BUILDING, AUDIT_GUIDE, MEMORY | 6 |
| "compose, don't reimplement" rule | CLAUDE.md, MANDATORY, BUILDING, AUDIT_GUIDE, MEMORY | 5 |

**Total: 14 distinct facts, each duplicated 2-6 times.**

Every duplication is a future contradiction. When a rule changes, the probability of updating all copies is near zero across LLM sessions.

### Gap Analysis

**Missing documentation:**
1. **No subsystem index or navigation document.** NORNIR_ORGANIZATION.md tries to serve this role but also does 4 other things. A dedicated "where to find documentation on X" table does not exist.
2. **No document explains the quality pipeline (saga/syn) at the right level for a session that needs to understand it but is not building it.** QUICKSTART has a brief overview. SYN_DESIGN has full detail. Nothing exists in between.
3. **No document covers settings.json hook wiring.** The only place this is documented is buried in HOOK_DESIGN.md (lines 225-244). A session that needs to wire a new hook to Claude Code has to stumble onto that section.
4. **Behavioral norms for LLM sessions live only in MEMORY.md.** The "ownership rule" and "gleipnir checks are not surface-level" rules are critical behavioral corrections with no home in the workspace documentation.
5. **No changelog or historical record.** Completed plans and resolved audit findings have no dedicated place. IMPROVEMENT_PLAN.md is a completed plan that will accumulate siblings (improvement_plan_2.md, etc.) without structure.

### Root-Level Clutter Assessment

7 markdown files at root is 3-4 too many. The current files serve these roles:
- **Entry point**: CLAUDE.md (1 file)
- **Compliance gate**: MANDATORY_READ_BEFORE_CODING.md (1 file)
- **Conventions**: NORNIR_NAMING.md, NORNIR_ORGANIZATION.md, NORNIR_BUILDING_AND_COMPOSITION.md (3 files)
- **Overview**: QUICKSTART.md (1 file)
- **Audit methodology**: AUDIT_GUIDE.md (1 file)

The three conventions files could be one file. The overview file is redundant with the conventions. The audit guide could move to audit/. That would leave 3 root files: CLAUDE.md, MANDATORY, and a single consolidated conventions doc.

### LLM-Navigability Score

**Cold-start to productive work: 4 file reads** (CLAUDE.md -> MANDATORY -> NAMING -> ORGANIZATION). This is reasonable but:
- The 4th file (ORGANIZATION) is 365 lines and covers too many topics. An LLM that reads it will absorb the directory map but may lose the nuance of the composition patterns by the time it starts coding.
- The 3rd file (NAMING) contains crate inventories that are also in the 4th file. The redundancy wastes context window.
- BUILDING_AND_COMPOSITION.md is the document most relevant to "how do I write code correctly" but it is the 5th file in the reading order and is not even mentioned in CLAUDE.md.

**Finding specific information mid-session: Poor.** If a session needs to know how to wire a hook to settings.json, there is no index or navigation aid. The session must guess which file to read. The information is in HOOK_DESIGN.md (line 225), which no document references.
