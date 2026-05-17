use bridge_core::state::shared_mock_state;
use std::net::SocketAddr;

pub fn mock_state() -> bridge_core::SharedState {
    shared_mock_state()
}

pub async fn replay_pgxl(bind_addr: SocketAddr) -> anyhow::Result<()> {
    pgxl_emulator::run(bind_addr, mock_state()).await
}

pub async fn replay_tgxl(bind_addr: SocketAddr) -> anyhow::Result<()> {
    tgxl_emulator::run(bind_addr, mock_state()).await
}
