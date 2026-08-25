# Domain documentation

This repository uses a single-context domain-documentation layout.

## Before exploring

Read these sources when they exist and are relevant:

- `CONTEXT.md` at the repository root.
- ADRs under `docs/adr/`.

Proceed silently when either source does not exist.
The `/domain-modeling` skill creates them only when terminology or a durable decision needs to be recorded.

## Use the glossary vocabulary

Use terms as defined in `CONTEXT.md` when naming issues, tests, types, and behavior.
Do not drift to synonyms the glossary explicitly avoids.

If a needed concept is absent, first decide whether the new term is necessary.
Record a genuine domain-language gap through `/domain-modeling`.

## Flag ADR conflicts

Surface any conflict with an existing ADR instead of silently overriding it.
