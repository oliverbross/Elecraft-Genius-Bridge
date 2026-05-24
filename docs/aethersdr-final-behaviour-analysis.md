# AetherSDR Final Behaviour Analysis

Phase 65 reviewed the AetherSDR source cloned under `.tmp/AetherSDR` and
compared it with live evidence from `20260523-131309-evidence-test`.

## TGXL Connects Immediately

AetherSDR has two TGXL paths:

- Flex/radio-side tuner model updates.
- Direct TGXL TCP on port 9010.

The public wiki describes the direct TGXL path: AetherSDR opens TCP port 9010
and the Tuner applet uses this connection for status, Tune, and relay actions.
In source, `TgxlConnection` sends `info` and `status` after the `V...` greeting,
then polls. `TunerModel::autoTune()` prefers the direct TGXL connection when it
is connected.

EGB’s direct TGXL path is stable and Tune reaches EGB as `autotune`, which maps
to KAT500 `T;` when enabled.

## PGXL Auto-Open Trigger

The direct PGXL socket opens from two code paths:

1. Flex amplifier detection:

   `.tmp/AetherSDR/src/gui/MainWindow.cpp` connects
   `RadioModel::amplifierChanged(bool)` to:

   `if (present && !m_radioModel.ampIp().isEmpty() && !m_pgxlConn.isConnected()) m_pgxlConn.connectToPgxl(m_radioModel.ampIp());`

2. Manual peripheral setting:

   AetherSDR reads `PGXL_ManualIp` and `PGXL_ManualPort` after radio settings
   load and calls `connectToPgxl()` if a manual IP is configured.

`RadioModel` sets `m_ampIp` only on the first non-TGXL `amplifier <handle> ...`
status with a non-empty model. Later amplifier statuses update telemetry but do
not re-emit `amplifierChanged(true)` just because the IP changes.

## Root Cause Of The 30-40s PGXL Delay

Latest evidence showed:

- EGB PGXL listener ready: `127.0.0.1:9008`.
- EGB advertised Flex amplifier IP: `192.168.0.189`.
- AetherSDR direct clients eventually connected from `127.0.0.1`.
- `pgxl-self-probe` proved the direct PGXL server responded immediately.

That means EGB was ready, but AetherSDR’s Flex-triggered auto-open was pointed at
the advertised LAN IP while the listener was bound only to loopback. The later
PGXL connection was the manual/local direct path, not the immediate Flex
amplifier auto-open.

Fix:

- The local band-follow profile now advertises `127.0.0.1` when the listener is
  bound to `127.0.0.1`.
- Operational startup now warns when the bridge binds PGXL to loopback while
  advertising a non-loopback amplifier IP.

Expected result:

- For same-host AetherSDR, PGXL should open from the Flex amplifier status in
  under 5 seconds.
- For remote AetherSDR, bind EGB to the LAN IP and advertise that same LAN IP.

Live Phase 65 check:

- With `force_advertised_pgxl_ip=127.0.0.1`, PGXL accepted about 0.2 seconds
  after the listener became ready.
- That validates the address-path fix and removes the need for additional PGXL
  listener timing hacks.

## AMP Button Command Path

The Amp applet button itself emits `operateToggled(bool)`. However, AetherSDR
does not send a direct PGXL operate command. Source shows the button maps to:

`amplifier set <ampHandle> operate=<0|1>`

through the Flex radio API, and only if `m_radioModel.ampHandle()` is not empty.

Consequences:

- PGXL direct TCP status can show telemetry, but it is not the command path for
  AetherSDR standby/operate.
- EGB can execute simulated standby and operate mapping, but it cannot execute a
  command that AetherSDR does not emit.
- If `flex-control-commands.log` and `pgxl-control-commands.log` are empty after
  a click, the client did not send a command to EGB.

Actionable verification after this phase:

1. Start EGB with the local AetherSDR profile.
2. Confirm `/status.flex_injection.amplifier_handle` is non-empty.
3. Click the actual Amp applet operate/standby button, not only the direct
   peripheral Connect button.
4. Check `flex-control-commands.log` for `amplifier set <handle> operate=...`.

## Meter And Interlock Effects

The AetherSDR source updates Amp applet telemetry from either direct PGXL status
or Flex amplifier telemetry. It does not require non-zero meters before showing
the button. Interlock-disabled tests did not make commands appear, so interlock
is not the primary no-command cause. The command path is gated by AetherSDR
having a Flex amplifier handle and by the user action being routed through the
Amp applet.
