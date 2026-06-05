# BUG-0011 NSCursor Provider Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix BUG-0011 so beautified export cursor shape follows the cursor macOS is actually displaying, including Arrow during Mission Control.

**Architecture:** Keep the existing cursor metadata pipeline (`MacCursorSource -> CursorMetadataRecorder -> CursorEffectEngine -> CursorOverlayRenderer`). Replace the production cursor kind source from Accessibility role inference to `NSCursor.currentSystemCursor`, mapping known system cursor instances to `CursorKind`. Unknown/read failures fall back to Arrow to avoid false Hand/IBeam classifications.

**Tech Stack:** Rust, AppKit FFI via Objective-C runtime calls, existing `CursorKindProvider` trait, cargo unit tests, Vitest UI tests for permission copy changes.

---

## File Structure

- Modify `src-tauri/src/platform/macos/cursor_kind.rs`: add NSCursor reader, pure mapping tests, production provider priority.
- Modify `src-tauri/src/platform/macos/cursor_source.rs`: update comments to reflect NSCursor-based provider.
- Modify `src-tauri/src/app/permission_service.rs`, `src-tauri/src/app/events.rs`, `src/lib/tauri.ts`, `src/App.tsx`: remove misleading Accessibility-required wording for cursor kind.
- Modify `BUG.md`, `HANDOFF.md`: record BUG-0011_9 root cause, fix, and manual gates.
- Add `tests/2026-06-05-bug-0011-nscursor-provider-checklist.md`: phase checklist.

## Phase 1: Planning And Checklist

- [ ] Add this plan.
- [ ] Add phase self-test checklist under `tests/`.
- [ ] Confirm no dirty worktree before implementation.

## Phase 2: Red Tests

- [ ] Add Rust tests in `cursor_kind.rs` for pure mapping:
  - `SystemCursorShape::Arrow -> Some(CursorKind::Arrow)`
  - `SystemCursorShape::PointingHand -> Some(CursorKind::Hand)`
  - `SystemCursorShape::IBeam -> Some(CursorKind::IBeam)`
  - unknown NSCursor shape does not become Hand.
- [ ] Add Rust tests for provider priority using a fake system cursor reader:
  - NSCursor Arrow wins even if AX would have inferred Hand.
  - NSCursor unknown falls back to Arrow.
- [ ] Add/update frontend test if removing the Accessibility warning changes visible UI.
- [ ] Run targeted tests and confirm they fail before production implementation.

## Phase 3: Implementation

- [ ] Implement `SystemCursorShape` and `system_cursor_shape_to_kind`.
- [ ] Add AppKit FFI helper that reads `[NSCursor currentSystemCursor]` inside an autorelease pool.
- [ ] Compare the current system cursor with `arrowCursor`, `pointingHandCursor`, and `IBeamCursor` via pointer equality / `isEqual:` first, then image + hot spot matching.
- [ ] Change `MacCursorKindProvider::query()` to prefer system cursor mapping and fallback to Arrow for unknown/read-failure.
- [ ] Keep AX functions available only as diagnostic/reference code; they must not be the production source that can emit false Hand.
- [ ] Update misleading Accessibility UI/comment copy.

## Phase 4: Verification And Docs

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --lib cursor_kind`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --lib`.
- [ ] Run `cargo fmt --check --manifest-path src-tauri/Cargo.toml`.
- [ ] Run `npm test -- --run`.
- [ ] Update `BUG.md` and `HANDOFF.md` with root cause, fix, prevention rules, and manual verification gates.

## Manual Gate

- [ ] Normal desktop/empty area: exported beautified cursor is Arrow.
- [ ] WebView/browser link or button: exported beautified cursor is Hand.
- [ ] Editable text field: exported beautified cursor is IBeam.
- [ ] Four-finger swipe into Mission Control: macOS shows Arrow and exported beautified video also shows Arrow.
- [ ] Diagnostics show nonzero Arrow/Hand/IBeam counts according to the actual recording path; no Accessibility permission is required for NSCursor-based cursor kind.
