# Interlock Disable Test Mode

`flex_injection.disable_amp_interlock: true` is a diagnostic-only mode.

When enabled:

- EGB still creates the virtual PGXL amplifier and AMP meters.
- EGB skips `interlock create type=AMP ...`.
- EGB does **not** send KPA500 `^OS1;`.
- EGB does **not** relax the KPA500 RF-risk gate.
- `/status.flex_diagnostics.interlock_disabled_for_test` reports `true`.

Purpose:

- Determine whether SmartSDR/AetherSDR transmit failures are caused by the virtual AMP interlock object.
- Compare TX/control behavior with and without the interlock while keeping the real KPA500 protected.

Do not use this mode as a normal operating profile. If TX becomes possible only with this mode, the next fix should be the interlock registration/association, not permanent interlock suppression.
