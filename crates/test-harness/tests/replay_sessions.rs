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
            "R2|0|state=STANDBY peakfwd=-120.0000 swr=-30.0000 temp=32.0 id=0.0 vac=0 meffa=OK fault= connection_state=connected\n",
            "R3|0|state=STANDBY peakfwd=-120.0000 swr=-30.0000 temp=32.0 id=0.0 vac=0 meffa=OK fault= connection_state=connected\n",
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
            "R1|0|info serial=EGB-TGXL serial_num=EGB-TGXL version=0.1.0-egb-tgxl firmware=0.1.0-egb-tgxl nickname=Tuner_Genius_XL 3way=1 model=TunerGeniusXL one_by_three=1 capabilities=direct_tcp,status,autotune,ant,manual_tune,flexradio,catradio,setup\n",
            "S2|status fwd=-120.00 peak=-120.00 max=0.00 swr=-30.0000 pttA=0 bandA=20 modeA=1 flexA=FlexRadio freqA=14.200000 bypassA=0 bypassRxA=0 antA=0 pttB=0 bandB=0 modeB=0 flexB= freqB=0.000 bypassB=0 bypassRxB=0 antB=0 state=0 active=1 tuning=0 bypass=0 ag=0 relayC1=20 relayL=35 relayC2=20 connection_state=connected fault=\n",
            "R3|0|\n",
            "S0|status fwd=-120.00 peak=-120.00 max=0.00 swr=-30.0000 pttA=0 bandA=20 modeA=1 flexA=FlexRadio freqA=14.200000 bypassA=0 bypassRxA=0 antA=0 pttB=0 bandB=0 modeB=0 flexB= freqB=0.000 bypassB=0 bypassRxB=0 antB=0 state=0 active=1 tuning=0 bypass=0 ag=0 relayC1=20 relayL=35 relayC2=20 connection_state=connected fault=\n",
            "R4|0|\n",
            "S0|status fwd=-120.00 peak=-120.00 max=0.00 swr=-30.0000 pttA=0 bandA=20 modeA=1 flexA=FlexRadio freqA=14.200000 bypassA=0 bypassRxA=0 antA=0 pttB=0 bandB=0 modeB=0 flexB= freqB=0.000 bypassB=0 bypassRxB=0 antB=0 state=0 active=1 tuning=0 bypass=0 ag=0 relayC1=20 relayL=35 relayC2=20 connection_state=connected fault=\n",
            "S5|status fwd=-120.00 peak=-120.00 max=0.00 swr=-30.0000 pttA=0 bandA=20 modeA=1 flexA=FlexRadio freqA=14.200000 bypassA=0 bypassRxA=0 antA=0 pttB=0 bandB=0 modeB=0 flexB= freqB=0.000 bypassB=0 bypassRxB=0 antB=0 state=0 active=1 tuning=0 bypass=0 ag=0 relayC1=20 relayL=35 relayC2=20 connection_state=connected fault=\n",
        )
    );
}
