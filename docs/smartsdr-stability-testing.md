# SmartSDR Stability Testing

Use `evidence-test` when SmartSDR shows repeated ATU connected/disconnected notifications.

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.flex-injection-readonly.yaml --duration-minutes 10
```

Keep SmartSDR connected for the whole run. If the ATU appears/disappears, EGB records Flex-side tuner lifecycle events in `disconnect-events.jsonl`.

For a SmartSDR-focused run, use `config.smartsdr-stability.yaml` or copy its settings into the local Flex profile:

```yaml
tgxl:
  smartsdr_compat: true
  experimental_presence_refresh: true

flex_injection:
  tuner_refresh_interval_ms: 5000
```

The refresh mode sends a rate-limited `sub amplifier all` query through the Flex API. It is diagnostic and does not create duplicate tuner objects.

Relevant event names:

- `tuner_registered`
- `smartsdr_tuner_disconnected`
- direct PGXL/TGXL `client_connected`
- direct PGXL/TGXL `client_disconnected`
- `tuner_presence_refreshed`

If no direct PGXL/TGXL client connects during the test, `summary.md` records a warning. That does not invalidate a Flex-side SmartSDR test, but it means the direct TCP path was not exercised.
