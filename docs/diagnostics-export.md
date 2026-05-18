# Diagnostics Export

The GUI `Export Diagnostics Bundle` button writes:

```text
diagnostics\egb-diagnostics-YYYYMMDD-HHMMSS.zip
```

Current contents:

- Active config YAML
- GUI settings YAML
- `/status` JSON snapshot if available
- Visible GUI/bridge logs
- Full current run log files from `logs\`
- GUI test/probe command output logs from `logs\tests`
- Windows version info
- Serial port list
- Protocol transcripts from `logs\protocol`
- Serial transcripts from `logs\serial`

By default, the GUI redacts:

- `bind_ip`
- `radio_ip`
- `amplifier_ip`
- `token`

Disable redaction only for local troubleshooting when you intend to share exact network details.

Every GUI test/probe command writes a separate file:

```text
logs\tests\YYYYMMDD-HHMMSS-<test-name>.log
```

Each test log includes the command, duration, stdout/stderr, and exit status.
