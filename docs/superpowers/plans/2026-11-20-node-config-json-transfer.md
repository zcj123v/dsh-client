# Node JSON Transfer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Transfer complete client node configuration and import curated Token site exports.

**Architecture:** A bounded versioned JSON codec validates whitelisted fields above the existing connection model. A preview prepares a batch upsert and separates volatile tokens from records. The site generates the same format from authenticated data and explicit SSH metadata.

**Tech Stack:** React, TypeScript, Vitest, Tauri, Rust, static HTML/JS.

**Spec:** `docs/superpowers/specs/2026-11-18-node-config-json-transfer-design.md`

## Global Constraints

Only the optional DSH login token travels as a credential. Export defaults exclude it. Import cannot connect, delete omitted nodes, or assign imported runtime partition identifiers. Keep HTTPS and SSH host-key policies. Android retains unsupported endpoints for round trips and attempts only HTTPS.

## Review Focus

Malformed input, atomic writes, duplicate keys, credential scope across endpoint changes, active connections, Android compatibility, website field whitelisting.

## Task 1 — Client codec and storage

- [ ] Add red tests in `tests/config-transfer.test.ts` covering round trip, strict validation, duplicates, scope change, atomic store update and failure.
- [ ] Add `src/config-transfer.ts` with `parseConfig`, `exportConfig`, `prepareImport` and `endpointFingerprint`. Preserve optional `kind` and stable `key` in `src/connection-model.ts` and `src/connection-store.ts`. Add one batched persistence action.
- [ ] Run focused tests and typecheck.

## Task 2 — Client interface and native connection integration

- [ ] Add red UI tests for preview/confirmation, same-format edit, token switch, and Android filtering.
- [ ] Add `src/ConfigTransfer.tsx` and wire through `src/App.tsx`. Existing loaded views are closed before changing their transport settings or updates are blocked until closed. Maintain a separate in-memory imported token map with explicit export and clear controls. Existing manually typed one-time token behavior remains intact.
- [ ] Extend native Android endpoint selection as needed and run Rust regressions. Verify native import/export I/O on available devices.

## Task 3 — Website exporter and integration

- [ ] Add fixture tests in the operations repository before implementing the Token page exporter and explicit SSH source metadata.
- [ ] Verify website JSON with the client parser. Review final differences including deployment boundaries. Run frontend tests, typecheck, build, Rust tests and available native packaging. Record what was tested and provide artifacts.
