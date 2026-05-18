# Dependency Review

Phase 22 reviewed GUI and diagnostics dependencies.

## `eframe = 0.27`

Retained for this phase. Upgrading egui/eframe can affect window lifecycle, tray integration options, widget APIs, and visual styling. Phase 22 is focused on SmartSDR stability and diagnostics correctness, so a GUI framework upgrade is higher risk than the value it provides right now.

Planned follow-up: evaluate the latest `eframe` in a dedicated GUI-only branch.

## `zip = 0.6`

Retained for this phase. The diagnostics exporter uses a small, stable API surface: create a ZIP, add text files, and add transcript files.

Planned follow-up: upgrade `zip` with a diagnostics bundle regression test that opens the generated ZIP and verifies required entries.

## Policy

Avoid broad dependency upgrades during live radio/control stability work unless a security advisory or build break requires it.
