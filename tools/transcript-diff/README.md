# Transcript Diff Tool

`transcript_diff.py` compares two protocol transcript files and highlights:

- missing or extra lines
- changed command/response ordering
- missing or changed key/value fields
- timing deltas between adjacent lines

It is a development-only helper for the AetherSDR correction loop. It is not part of the deployed bridge binary.

Example:

```powershell
python tools\transcript-diff\transcript_diff.py `
  --expected tests\replay\pgxl-polling-session.txt `
  --actual logs\protocol\pgxl-SESSION.log
```

Use captured AetherSDR transcripts as `--actual`. Use replay fixtures or previously accepted transcripts as `--expected`.
