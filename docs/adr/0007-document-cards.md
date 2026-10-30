# ADR 007: Document cards

## Status

Accepted

## Context

A scannable library needs more than filenames. Users want type, date, people, a short summary, and grounded facts before they ask a question. Running the full answer model on every document would compete with chat and burn local GPU time.

## Decision

After a document is indexed, a background worker makes one checked model call with the configured background model and stores a document card (type, title, date, people, organisations, summary sentences, key facts, typed fields, aliases, suggested questions). Every summary sentence, fact, and field carries quotes that are verified against retrieved passages. The library list shows card metadata; opening a row loads the card. Suggested questions for empty chat come from cards. Graph extraction stays optional and off by default.

## Consequences

Cards finish in the background without blocking chat. Failed cards leave the document indexed and searchable. Changing the background model or turning summarise off or on is a Settings change; turning summarise on again requeues pending cards.
