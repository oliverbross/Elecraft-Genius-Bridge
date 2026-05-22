# AetherSDR Compatibility Profile

`official_pgxl` emits the documented Flex amplifier create line:

```text
amplifier create ip=<egb-lan-ip> port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB
```

Latest live evidence showed Flex accepts this strict command, but AetherSDR did not open the PGXL/TGXL direct TCP sessions before the radio removed the virtual amplifier object. That makes `official_pgxl` useful for protocol audit work, but too strict for the current AetherSDR operational path.

`aethersdr_operational` is the normal AetherSDR profile. It keeps all Phase 49 safety gates and adds only the direct-connect readiness fields that the older working profile used:

```text
amplifier create ip=<egb-lan-ip> port=9008 model=PowerGeniusXL serial_num=EGB-KPA500 ant=ANT1:PORTA,ANT2:PORTB state=<live-kpa-state> connected=1 configured=1 enabled=1 direct=1 lan=1
```

The `state` value is generated from live KPA500 polling. It must not be hard-coded. If the KPA500 preflight fails, EGB does not advertise PGXL as healthy.

`aethersdr_operational` differs from old lab profiles:

- `pgxl_connect_assist` remains disabled.
- EGB does not send `amplifier set <handle> operate=1` to force a UI state.
- KPA500 preflight, KAT500 preflight, stale-binary checks, and LAN advertised-IP checks still run.
- RF-risk commands remain disabled unless explicitly enabled by operational policy.

Recommended AetherSDR live-test config:

```text
config.aethersdr-compatible-operational.yaml
```

Use `config.aethersdr-real-operational.yaml` only for strict official PGXL/Flex registration audits.
