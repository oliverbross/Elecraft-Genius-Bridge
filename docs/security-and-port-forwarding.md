# Security And Port Forwarding

The MVP raw PGXL/TGXL emulator ports do not implement authentication or TLS.

Do not expose these ports directly to the public internet:

- PGXL: TCP `9008`
- TGXL: TCP `9010`

Recommended MVP access methods:

- Same LAN.
- Tailscale.
- ZeroTier.
- VPN.
- A controlled reverse proxy or tunnel that adds access policy.

Reserved future security fields exist in config, but token authentication is not applied to the native PGXL/TGXL wire protocol because AetherSDR expects the real device protocol.

Future security work:

- Authenticated web/API control plane.
- Optional TLS wrapper where clients support it.
- IP allowlists.
- Rate limits.
- Better malformed-packet accounting.

