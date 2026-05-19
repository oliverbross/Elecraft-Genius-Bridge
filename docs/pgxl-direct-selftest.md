# PGXL Direct Self-Test

Use this to prove EGB is listening on TCP `9008` independently of AetherSDR:

```powershell
.\target-msvc\debug\egb.exe test-pgxl-direct --host 127.0.0.1 --port 9008
```

Expected sequence:

- connect TCP
- receive `V...`
- send `C1|info`
- send `C2|status`
- send `C3|standby`
- send `C4|operate`

The direct `operate` command only updates desired state in the direct protocol layer. Real RF-risk KPA500 operate remains gated elsewhere by config and explicit CLI flags.

Output is written to:

```text
logs/tests/YYYYMMDDHHMMSS-pgxl-direct-selftest.log
```

If this passes but AetherSDR still shows PGXL not connected and `pgxl_session_started_count=0`, the problem is not the socket listener. It means AetherSDR did not attempt TCP `9008`.
