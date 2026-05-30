# Phase 6 / W11-W12 验收清单

## Verification Summary (2026-05-30, Phase 6)

- `git diff --check`: PASS
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml`: PASS, 198 tests
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`: PASS (warnings only, no errors)
- `cargo build --manifest-path src-tauri/Cargo.toml`: PASS
- `npm run build`: PASS
- `npm test -- --run`: PASS, 51 tests
- `cargo test --manifest-path src-tauri/Cargo.toml --features ffmpeg`: BLOCKED (FFmpeg dev libraries not available in current environment)
- Manual FFmpeg export: BLOCKED (requires FFmpeg dev libraries and Native Safety review)
- Native Safety Gate: BLOCKED (requires human reviewer)

## Automated Checks

- [x] Export presets have fixed MVP dimensions (1920x1080, 1080x1920, 1080x1080)
- [x] Export preset parse rejects unknown values
- [x] Export path is separate from source path
- [x] Non-empty output requires existing non-empty file
- [x] Base audio activity analyzer emits 100ms buckets
- [x] Aggregate base buckets uses current sensitivity window
- [x] Base analyzer accumulates short chunks before emitting bucket
- [x] Trim metadata round trips JSON with schema version
- [x] Export service rejects missing source artifact
- [x] Export service sends structured request to exporter
- [x] Export progress serializes camelCase
- [x] Export error uses Chinese message
- [x] Export cancelled uses Chinese message
- [x] License service first status starts 14-day trial
- [x] License service trial expires after 14 days
- [x] License service activated status overrides trial expiry
- [x] License service file trial state rejects activation entitlement
- [x] Source artifact requires existing non-empty file
- [x] Source artifact error mentions original recording

## Frontend Checks

- [x] Shows playable export success when outputPath is returned
- [x] Can request export cancellation from preview
- [x] Shows local trial days in idle state
- [x] Shows expired local trial state
- [x] Shows activated local license state
- [x] Export progress bar visible during export
- [x] Cancel export button visible during export
- [x] Export buttons disabled during export

## FFmpeg Artifact Evidence

| Scenario | Source path | Output path | Output bytes | Width x Height | Video stream | Audio stream | Source duration | Output duration | Duration delta | Original exists before/after | Inspector | Date |
|---|---|---|---:|---|---|---|---:|---:|---:|---|---|---|
| original recording artifact |  | n/a |  |  |  |  |  | n/a | n/a | n/a |  |  |
| 16:9 auto-trim off |  |  |  | 1920x1080 |  |  |  |  |  |  |  |  |
| 9:16 auto-trim off |  |  |  | 1080x1920 |  |  |  |  |  |  |  |  |
| 1:1 auto-trim off |  |  |  | 1080x1080 |  |  |  |  |  |  |  |  |
| 16:9 auto-trim on |  |  |  | 1920x1080 |  |  |  |  |  |  |  |  |
| cancel cleanup |  |  | n/a | n/a | n/a | n/a |  | n/a | n/a |  |  |  |

## Manual Gates

- [ ] Bilibili / YouTube 16:9 export opens and contains video/audio.
- [ ] Douyin 9:16 export opens and contains video/audio.
- [ ] Xiaohongshu 1:1 export opens and contains video/audio.
- [ ] Auto-trim off exports full source.
- [ ] Auto-trim on consumes CutTimeline and produces shorter output when cuts exist.
- [ ] Binding inspection records dimensions, duration, video stream, audio stream, and file size for original/export artifacts.
- [ ] Export progress contains at least one intermediate 1-99 value from exporter callback.
- [ ] Original recording artifact still exists after export.
- [ ] Export cancel removes partial output.
- [ ] Export failure returns Chinese structured error and does not return fake outputPath.
- [ ] License badge does not overlap controls at 360px, 768px, or desktop widths.
- [ ] 10-minute 1080p recording/export pressure check records stop time, sidecar size, memory peak, and export time.
- [ ] Native Safety review covers ffmpeg_writer.rs, trim_exporter.rs, SCK callback, and credential persistence.
- [ ] BUG.md prevention scan passes.

## Implementation Notes

### What was completed (Tasks 1-8)

1. **Export Presets**: Three fixed presets (Bilibili 16:9, Douyin 9:16, Xiaohongshu 1:1) with deterministic output paths.
2. **Base RMS Buckets**: Sensitivity-independent 100ms base audio activity buckets for post-recording re-aggregation.
3. **Export Service Boundary**: Structured export request with source validation, cancel token, and progress reporter.
4. **Export Progress & Cancel**: ExportProgressPayload events, cancel_export command, UI progress bar and cancel button.
5. **Preview Export UI**: Progress bar, cancel button, playable output message, disabled buttons during export.
6. **Original Recording Artifact**: Source artifact validator, FFmpeg test support helpers.
7. **FFmpeg Export Gate**: FfmpegTrimExporter with proper validation (actual transcoding requires Native Safety review).
8. **License Service**: 14-day local trial, activation status interface, LicenseStatus component in UI.

### What requires human review (Tasks 6A, 6)

- FFmpeg recording writer implementation (real encoding/muxing)
- FFmpeg trim exporter implementation (real transcoding with cut timeline)
- Native Safety review for all FFmpeg binding code
- macOS Keychain / Windows Credential Manager for activation persistence

### Remaining work for future phases

- Real FFmpeg transcoding implementation (requires Native Safety review)
- Server activation protocol
- Activation code validation
- Long recording pressure tests
- A/V sync verification
