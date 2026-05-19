# SmartSDR Stability Testing

Use `evidence-test` when SmartSDR shows repeated ATU connected/disconnected notifications.

```powershell
.\target-msvc\debug\egb.exe evidence-test --config .\config.flex-injection-readonly.yaml --duration-minutes 10
```

Keep SmartSDR connected for the whole run. If the ATU appears/disappears, EGB records Flex-side tuner lifecycle events in `disconnect-events.jsonl`.

Relevant event names:

- `tuner_registered`
- `smartsdr_tuner_disconnected`
- direct PGXL/TGXL `client_connected`
- direct PGXL/TGXL `client_disconnected`

If no direct PGXL/TGXL client connects during the test, `summary.md` records a warning. That does not invalidate a Flex-side SmartSDR test, but it means the direct TCP path was not exercised.
