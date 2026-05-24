# Flex Amplifier State Reannounce Mechanism

Phase 66 reviewed why `flex-tx.log` contained frequent repeated
`sub amplifier all` commands.

## Finding

The Flex TCP API path used by EGB has a verified subscription/query command:

`sub amplifier all`

No separate verified external-client command has been proven that pushes a
synthetic amplifier status update into the radio. Therefore EGB cannot directly
force the radio to emit arbitrary amplifier state; it can only keep the
registered object stable and request amplifier status through subscription/query
traffic.

## Change

EGB no longer sends duplicate periodic `sub amplifier all` refreshes from both
the amplifier and tuner refresh paths.

Kept:

- initial registration subscription;
- bounded startup burst for AetherSDR direct-connect timing;
- one amplifier keepalive refresh at the configured interval;
- explicit state-change burst after KPA500 `^OS` changes;
- normal Flex ping/keepalive.

Removed/reduced:

- periodic tuner presence subscription spam.

Live Phase 66 testing showed that removing the amplifier refresh entirely caused
Flex to remove the amplifier object after about 15 seconds. The remaining single
amplifier refresh is therefore treated as a required keepalive/reannounce
heartbeat, not optional spam.

## State Change Burst

When KPA500 `^OS` changes, EGB:

1. Updates shared KPA/PGXL state immediately.
2. Updates PGXL direct status from the shared state.
3. Requests a bounded Flex amplifier refresh burst at approximately:

`0 ms, 250 ms, 500 ms, 1000 ms, 1500 ms, 2000 ms`

4. Returns to normal cadence.

The burst is logged in:

- `kpa-state-reannounce.log`
- `amplifier-reannounce.log`
- `flex-tx.log`

## Expected Behaviour

- AetherSDR direct PGXL telemetry should reflect the new state on its next direct
  `status` poll after EGB detects the KPA `^OS` change.
- Flex-side fallback telemetry should update during the bounded burst.
- `flex-tx.log` should contain the startup burst, the bounded state-change burst,
  and one configured amplifier keepalive refresh. It should not contain a second
  periodic tuner refresh stream.
