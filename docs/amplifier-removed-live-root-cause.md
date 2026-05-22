# Amplifier Removed Live Root Cause

`amplifier <handle> removed` is a primary Flex lifecycle failure, not a warning.

When Flex reports removal, EGB now writes removal evidence containing:

- the last 50 Flex RX/TX lines,
- last emitted amplifier create/status line,
- last Flex radio amplifier status line,
- last PGXL advertised state,
- latest KPA500 poll snapshot,
- duplicate create/subscription counts,
- meter and interlock handles,
- command response state.

The expected stable lifecycle is:

1. receive Flex `H<client-handle>`;
2. send one `amplifier create ip=<egb-lan-ip> port=9008 model=PowerGeniusXL serial_num=<serial> ant=<map>`;
3. send AMP meter creates for FWD/RL/DRV/ID/TEMP;
4. send AMP interlock create;
5. enable keepalive;
6. subscribe amplifier/slice/TX status;
7. keep the amplifier handle until the Flex TCP session itself reconnects.

EGB must not use rejected `amplifier set <handle> operate=1` commands as lifecycle machinery. `pgxl_connect_assist` remains a compatibility workaround only and is off in the base config defaults.

The next live run should inspect this file from the evidence bundle. If removal occurs, the preceding Flex RX/TX lines should identify whether Flex removed the object because of duplicate registration, rejected lifecycle commands, keepalive loss, invalid interlock/meter setup, or invalid amplifier identity fields.
