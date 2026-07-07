# polyplug Documentation Style

The voice and word-choice standard for everything under `docs/` and the project's
READMEs. It is prescriptive: when a page disagrees with this file, fix the page.

## Voice

- **Second person, active voice, present tense.** "You register the contract," not
  "the contract is registered" or "we will register the contract."
- **No throat-clearing.** Do not open with an "Overview" section that restates the
  page title, and do not preface content with "This document describes…". Start with
  the first useful sentence.
- **Say it once.** State a fact in the one place it belongs and link to it elsewhere;
  do not restate it.

## Page types

Every page belongs to exactly one part. The part fixes the page's mode and heading scheme,
so the choice is mechanical — never guess:

| Part | Mode | Heading scheme |
|---|---|---|
| Getting Started | learn — one happy path, hand-held | `## N. Verb` |
| Guides | do a task — terse, **zero internals** | `## N. Verb` |
| Reference | look up — dry, complete, scannable | topic nouns |
| Concepts | understand — the **only** home for internals | topic nouns |
| Operations | run it — performance, profiling, crashes | topic nouns |
| Security & Trust | trust boundaries, disclosure | topic nouns |
| Project | meta — workflow, examples | topic nouns |

## Headings

- **Sequential guides** use `## N. Verb phrase` — numbered, imperative: `## 1. Define
  the contract`, `## 2. Generate the bindings`.
- **Reference and concept pages** use topic nouns: `## HostApi`, `## Memory layout`,
  `## Dispatch families`.
- One scheme per page. Do not mix numbered steps with topic nouns in the same page.

## Guides contain zero internals

- A guide tells the reader what to do, never how the runtime works inside. When a step
  begs a "why," link to the relevant Concepts page instead of explaining it inline.
- If a guide cannot avoid an internal detail, that detail belongs in Concepts and the
  guide links to it.

## Terms

- **Define each term once, in `docs/glossary.md`.** Never redefine a term inline and
  never carry a per-page "Terminology Note." Link to the glossary entry when first use
  needs disambiguation.
- Use the project's canonical nouns exactly (bundle, contract, guest contract, host
  contract, guest, host, loader, instance, descriptor, `HostApi`,
  `GuestContractInterface`, `StringView`, arena, epoch, revision counter, hot-reload,
  unload, peer dispatch).

## Code

- **Minimal, runnable blocks.** Show the smallest snippet that makes the point. Do not
  paste full files — link to the real source under `examples/` for the complete,
  compiling version.
- Prefer a `{{#include}}` of real source over a copied snippet when the whole thing is
  needed, so the docs cannot drift from the code.

## Format by purpose

- **Tables** for reference material (field layouts, option lists, matrices).
- **Prose** for concepts and rationale.
- **Numbered steps** for guides.

## Moving a page

- When you move a page, re-path every `{{#include}}` it contains **and** every
  `{{#include}}` that points at it.
- Verify the *rendered* page after the move (run `mdbook build` and open it), not just
  the link check — a broken include path can still produce a page, just an empty one.
