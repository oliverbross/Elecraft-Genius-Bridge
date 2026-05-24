# Flex Control Semantic Variants

Phase 69 adds isolated AetherSDR/Flex advertisement variants without changing the working baseline profile.

## Variants

| Variant | Create/status intent | Extra fields |
| --- | --- | --- |
| `no_hack_fields` | Use documented PGXL create fields and normal status evidence. | none |
| `state_only` | Add only live `state=<KPA state>`. | `state` |
| `current_hack_fields` | Reproduce old force-direct readiness fields for comparison. | `state`, `connected`, `configured`, `enabled`, `direct`, `lan` |

The normal `current` variant preserves the selected profile, including the known-good `aethersdr_force_direct` behaviour when explicitly configured.

## How To Compare

Run one variant at a time while AetherSDR is connected:

```powershell
.\target\release\egb.exe aethersdr-open-trigger-test --config .\config.aethersdr-last-known-good-real-controls.yaml --variant no_hack_fields --duration-seconds 120
.\target\release\egb.exe aethersdr-open-trigger-test --config .\config.aethersdr-last-known-good-real-controls.yaml --variant state_only --duration-seconds 120
.\target\release\egb.exe aethersdr-open-trigger-test --config .\config.aethersdr-last-known-good-real-controls.yaml --variant current_hack_fields --duration-seconds 120
```

Compare:

- PGXL direct connect delay.
- AMP button command emission.
- SmartSDR PGXL display behaviour.
- Interlock state.
- Meter handles and meter publication state.

## Interpretation

If `no_hack_fields` or `state_only` still connects immediately and command emission remains absent, the readiness fields are not the AMP command blocker. If only `current_hack_fields` connects quickly, those fields remain AetherSDR compatibility fields, but they should be treated as client-workaround fields rather than official PGXL semantics.
