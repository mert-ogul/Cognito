# ADR 007: Document cards

## Status

Accepted

## Context

A library of filenames alone is hard to scan. Users want type, date, people, a short summary, and grounded facts before they ask a question. Running the full answer model on every document would compete with chat and burn local GPU time.

## Decision

After a document is indexed, a background worker makes one checked model call with the configured model for document cards and stores a document card (type, title, date, people, organisations, summary sentences, key facts, typed fields, aliases, suggested questions). Every summary sentence, fact, and field carries quotes that are verified against retrieved passages. The library list shows card metadata; opening a row loads the card and related documents. Suggested questions for empty chat come from cards. Name extraction for connections is a separate toggle (on by default).

## Consequences

Cards finish in the background without blocking chat. Failed cards leave the document indexed and searchable. Changing the document-card model or turning summarise off or on is a Settings change; turning summarise on again requeues pending cards.
