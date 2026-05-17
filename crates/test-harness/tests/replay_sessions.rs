use bridge_core::state::shared_mock_state;

#[tokio::test]
async fn pgxl_polling_replay_is_stable() {
    let state = shared_mock_state();
    let transcript = include_str!("../../../tests/replay/pgxl-polling-session.txt");
    let mut output = Vec::new();
    for line in transcript.lines().filter(|line| !line.trim().is_empty()) {
        output.push(pgxl_emulator::replay_line(line, &state).await.unwrap());
    }
    assert_eq!(
        output.concat(),
        concat!(
            "R1|0|model=PowerGeniusXL serial_num=EGB-PGXL version=0.1.0-egb-pgxl firmware=0.1.0-egb-pgxl capabilities=direct_tcp,status\n",
            "R2|0|state=STANDBY peakfwd=0.0000 swr=1.0000 temp=32.0 id=0.0 vac=230 meffa=OK fault= connection_state=connected\n",
            "R3|0|state=STANDBY peakfwd=0.0000 swr=1.0000 temp=32.0 id=0.0 vac=230 meffa=OK fault= connection_state=connected\n",
        )
    );
}

#[tokio::test]
async fn tgxl_polling_replay_is_stable() {
    let state = shared_mock_state();
    let transcript = include_str!("../../../tests/replay/tgxl-polling-session.txt");
    let mut output = Vec::new();
    for line in transcript.lines().filter(|line| !line.trim().is_empty()) {
        output.extend(tgxl_emulator::replay_line(line, &state).await.unwrap());
    }
    assert_eq!(
        output.concat(),
        concat!(
            "R1|0|model=TunerGeniusXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl firmware=0.1.0-egb-tgxl one_by_three=1 capabilities=direct_tcp,status,autotune,ant,manual_tune\n",
            "R2|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=1.0000 connection_state=connected fault=\n",
            "R3|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=1.0000 connection_state=connected fault=\n",
            "S0|state operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=1.0000 connection_state=connected fault=\n",
            "R4|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=1.0000 connection_state=connected fault=\n",
            "S0|state operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=1.0000 connection_state=connected fault=\n",
            "R5|0|operate=0 bypass=0 tuning=0 relayC1=20 relayL=35 relayC2=20 antA=0 one_by_three=1 fwd=0.0000 swr=1.0000 connection_state=connected fault=\n",
        )
    );
}
