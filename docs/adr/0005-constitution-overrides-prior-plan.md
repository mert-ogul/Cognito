# ADR 005: Constitution overrides conflicting prior plans

## Status

Accepted

## Context

Older design notes and prototypes considered alternate stacks (different graph stores, packagers, or query routers). Those documents remain useful history but can contradict the current MVP.

## Decision

**[CONSTITUTION.md](../../CONSTITUTION.md) is authoritative.** Where an earlier plan conflicts with the constitution or with ADRs marked Accepted, the constitution and current ADRs win. Engineering work should not reintroduce dropped scope (for example Kuzu, PyInstaller-only packaging, or an adaptive query classifier for MVP) without a new ADR and constitution update.

## Consequences

- Onboarding reads the constitution first; stale docs are labeled or archived when they diverge.
- Release criteria and code review use constitution + ADRs as the contract.

## Related

- [Revised MVP stack](./0002-revised-mvp-stack.md)
