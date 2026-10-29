# ADR 004: Security and privacy boundaries

## Status

Accepted

## Context

Cognito reads personal documents from a folder the user chooses. The attack surface must stay small, and document content must not leave the machine except when the user points inference at a network Ollama host.

## Decision

The webview Content-Security-Policy allows only the app origin, inline styles needed by the shell, and images from https and data. Capabilities grant core invoke and events plus the folder dialog; there is no generic shell or filesystem API from the frontend. File access goes through Rust commands. The sidecar speaks JSON-RPC on stdin and stdout only. The only intentional network traffic is HTTP to the configured Ollama host. There is no telemetry or crash-reporting channel in the product.

## Consequences

Changing CSP, capabilities, or Ollama host behaviour needs an explicit product decision. Users who point Ollama at a remote host send prompts and embeddings there by choice.
