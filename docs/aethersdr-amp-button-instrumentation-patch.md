# AetherSDR AMP Button Instrumentation Patch

Live EGB evidence shows no usable AMP operate/standby command reaches EGB:

- `flex-control-commands.log` is empty.
- `pgxl-control-commands.log` is empty.
- Direct PGXL receives `info` and `status`, not control commands.
- EGB simulation proves KPA500 Standby can be mapped to `^OS0;` if a command
  arrives.

The inspected AetherSDR source path indicates the AMP button should emit a Flex
API command:

```cpp
connect(m_appletPanel->ampApplet(), &AmpApplet::operateToggled, this, [this](bool on) {
    if (!m_radioModel.ampHandle().isEmpty())
        m_radioModel.sendCommand(
            QString("amplifier set %1 operate=%2").arg(m_radioModel.ampHandle()).arg(on ? 1 : 0));
});
```

The next diagnostic step is to instrument AetherSDR itself, because EGB cannot
execute a command that AetherSDR does not emit.

## Suggested Local Diagnostic Patch

Add logging at the AMP applet click signal:

```cpp
connect(m_appletPanel->ampApplet(), &AmpApplet::operateToggled, this, [this](bool on) {
    qInfo() << "AmpApplet operateToggled" << on
            << "ampHandle" << m_radioModel.ampHandle()
            << "ampIp" << m_radioModel.ampIp();

    if (m_radioModel.ampHandle().isEmpty()) {
        qWarning() << "AmpApplet operate command blocked: empty ampHandle";
        return;
    }

    const auto command =
        QString("amplifier set %1 operate=%2").arg(m_radioModel.ampHandle()).arg(on ? 1 : 0);
    qInfo() << "AmpApplet emitting Flex command" << command;
    m_radioModel.sendCommand(command);
});
```

Also log the applet's enabled/clickable state near the button update path:

```cpp
qInfo() << "AmpApplet state"
        << "connected" << connected
        << "operate" << operate
        << "buttonEnabled" << ui->operateButton->isEnabled();
```

## What To Prove

The patch should answer:

- Was the visible AMP button actually clicked?
- Did `operateToggled(bool)` fire?
- Was `m_ampHandle` empty?
- Was `m_ampIp` present?
- Was the Flex command emitted?
- If not emitted, which UI eligibility condition blocked it?

If AetherSDR logs a command but EGB still does not see it in `flex-rx.log`,
the next suspect is the AetherSDR radio command transport. If AetherSDR never
logs the click or command, the issue is UI state/eligibility inside AetherSDR.

