# AetherSDR Setup

EGB targets AetherSDR direct manual IP configuration.

## Mock Mode

1. Start EGB:

```powershell
cargo run -p egb -- run --config config.yaml
```

2. In AetherSDR, open Radio Setup, then Peripherals.
3. Set Tuner Genius XL (TGXL):
   - IP address: bridge host IP
   - Port: `9010`
4. Set Power Genius XL (PGXL):
   - IP address: bridge host IP
   - Port: `9008`
5. Click Connect for each device.

Expected behavior:

- AetherSDR connects without a discovery step.
- EGB logs show `info` and repeated `status`.
- TGXL relay controls send `tune relay=...`.
- TGXL tune sends `autotune`.
- TGXL antenna buttons send `activate ant=N` when visible.

## Notes

The default `config.example.yaml` binds to `127.0.0.1`. For a separate Mac running AetherSDR, set `server.bind_ip` to the Windows machine LAN IP.

