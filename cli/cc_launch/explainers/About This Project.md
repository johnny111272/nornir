# About This Project

> "About six months in, I knew pretty well how to get Claude initialized — but it was freaking
> tedious. And I needed a different kind of Claude for different projects. One size fits all just
> isn't true."

## What problem does this solve?

Claude Code boots the same way for everyone: a vendor-written initialization aimed at coding —
task-completion, execution-first. The author's work doesn't fit one mode. It spans system
architecture, system design, and implementation — plus writing, planning, and security work — and
each needs a *different kind of Claude*: different thinking posture, different domain knowledge,
different languages, sometimes no coding reflexes at all. Under the one-size boot, sessions jump to
implementation during architecture conversations and grab the first viable solution during design.

By about six months of daily collaboration, the author had learned how to initialize a session
properly by hand. It worked — and it was tedious enough, session after session, project after
project, that the initialization itself clearly needed to become a tool.

## Why build one — can't you just tell Claude how to behave?

That was tried, in every form, and each fails for a structural reason:

- **Better prompts fade.** Instructions lose influence as a session fills; what guides behaviour is
  what's near in context, not what was said at the start. A two-thousand-token instruction cannot
  outweigh the statistical mass of the model's entire training.
- **More rules make it worse.** Each added rule dilutes attention across all of them — compliance
  drops as the rulebook grows.
- **Mid-session correction doesn't stick.** A session that starts misaligned resists being talked
  back into shape; explanations and examples fail against a badly configured starting state.
- **Thorough documentation backfires.** Coherent docs give the model the vocabulary to *sound*
  aligned while remaining wrong — confidence without comprehension.

The turn came from an experiment: a medieval story about a castle — gates, walls, covenant books —
outperformed weeks of explicit technical instruction at getting sessions to respect a system's
architecture. The lesson: initialization doesn't *transfer information*, it **configures the
landscape** the model works in — which patterns come easily and which stay out of reach. If that's
what initialization is, it deserves to be a first-class, deliberately composed artifact — not a
vendor default and not a hand-ritual.

## What's the idea?

A launcher. A library of small, composable XML fragments in categories — the collaboration
paradigm, thinking frameworks, behavioural rules, communication style, safety guardrails, platform
mechanics, personas, per-system and per-workspace descriptors, coding practice, domain expertise. A
terminal UI where the author picks the workspace, the persona, and what to preload; a profile
cascades sensible defaults; programming languages are *derived* from what's selected, never
configured by hand. The launcher assembles the fragments in a deliberate order — collaboration
first, persona early, because **position in a prompt is weight** — writes the result, and launches
Claude Code with it. *"The system prompt IS the expertise: a focused prompt produces a specialist."*

Personas do heavy lifting for free: naming a figure the model's training already knows deeply —
Norse archetypes, in this library — activates a whole behavioural posture in a single reference,
where spelling it out would cost thousands of fading tokens.

And the system is extensible in a specific, disciplined way: when a session discovers a genuinely
good rule of working, it goes first to a shared memory space; if it proves out, it is *promoted
into the standard initialization and removed from memory*. The boot sequence itself accumulates
what the collaboration learns.

## Who is it for?

The author, every single day — it saves hours: *"All I do is open cc_launch, figure out what
persona is going to be most helpful, where I'm going to be working, and what I want to preload —
and select it."* It was built for him, but the tool is genuinely universal: anyone could point it
at their own fragment library. What transfers is the method — initialization as a composed,
owned artifact; the library itself is personal by nature.

## Where does it fit?

It's a crate in the author's Rust infrastructure monorepo, and it reads the fragment library from
his central control area. In the larger collaboration system it is the **initialization** half of
the story — the guardrail pipeline being the **constraint** half. It is also the most recursive
project on this site: cc_launch booted the session that drafted this page.

## Where does it stand?

Live and in daily use, working well. Its design document publishes its own warts — a pending-work
list, including one module whose environment-variable names were invented by an earlier session
and await replacement with the real mechanism. One real limitation currently lives upstream: on
session resume and after context compaction, the platform no longer re-inlines a custom system
prompt — the model gets a summary and a file reference it rarely follows unprompted. The interim
fix is a bootloader note in shared memory that instructs each session to read its full assembled
prompt and *prove* it did; the permanent fix moves prompt injection into the author's traffic
layer. (The session writing this page passed that proof step.)

## What's here to read?

`CC_LAUNCH_DESIGN.md` — the real working design document, under its real filename: the assembly
order, the fragment categories, workspace profiles, the TUI, session lifecycle, and the honest
pending-work list.
