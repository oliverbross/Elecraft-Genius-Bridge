# SmartSDR TGXL Registration Limitation

SmartSDR frequency changes reach EGB through Flex slice tracking, and EGB can push KAT500 `F <kHz>;` accordingly. That proves the radio-context path is working.

What remains missing is a verified Flex API object model for registering an external Tuner Genius XL so SmartSDR shows a TGXL/tuner widget.

Current result:

- AetherSDR direct TGXL TCP is supported.
- SmartSDR frequency context is supported through Flex slice subscriptions.
- SmartSDR TGXL/tuner visibility remains unsupported until a documented or captured Flex tuner/accessory registration command is found.

EGB should not claim SmartSDR TGXL support yet. The next evidence needed is a real TGXL-to-Flex registration capture or official Flex API documentation for external tuner registration.
