use std::{path::PathBuf, sync::Arc};

use aide::axum::{
    routing::{get, post},
    ApiRouter,
};
use anyhow::Result;
use blockchain::BlockchainClient;
use clap::Parser;
use config::{load_config, CardanoConnectConfig};
use firefly_server::instrumentation;
use persistence::Persistence;
use routes::{
    chain::get_chain_tip,
    health::health,
    streams::{
        create_listener, create_stream, delete_listener, delete_stream, get_listener, get_stream,
        list_listeners, list_streams, update_stream,
    },
    transaction::submit_transaction,
    ws::handle_socket_upgrade,
};
use signer::CardanoSigner;
use streams::StreamManager;
use tracing::instrument;

mod blockchain;
mod config;
mod persistence;
mod routes;
mod signer;
mod streams;
mod utils;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(short = 'f', long)]
    pub config_file: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    pub blockchain: Arc<BlockchainClient>,
    pub signer: Arc<CardanoSigner>,
    pub stream_manager: Arc<StreamManager>,
}

#[instrument(err(Debug))]
async fn init_state(config: &CardanoConnectConfig) -> Result<AppState> {
    let persistence = Arc::new(Persistence::default());
    let blockchain = Arc::new(BlockchainClient::new(config).await?);

    let state = AppState {
        blockchain: blockchain.clone(),
        signer: Arc::new(CardanoSigner::new(config)?),
        stream_manager: Arc::new(StreamManager::new(persistence, blockchain).await?),
    };

    Ok(state)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let config_file = args.config_file.as_deref();
    let config = load_config(config_file)?;

    instrumentation::init(&config.log)?;

    let state = init_state(&config).await?;

    let router = ApiRouter::new()
        .api_route("/api/health", get(health))
        .api_route("/api/transactions", post(submit_transaction))
        .api_route("/api/eventstreams", post(create_stream).get(list_streams))
        .api_route(
            "/api/eventstreams/:streamId",
            get(get_stream).patch(update_stream).delete(delete_stream),
        )
        .api_route(
            "/api/eventstreams/:streamId/listeners",
            post(create_listener).get(list_listeners),
        )
        .api_route(
            "/api/eventstreams/:streamId/listeners/:listenerId",
            get(get_listener).delete(delete_listener),
        )
        .api_route("/api/chain/tip", get(get_chain_tip))
        .route("/api/ws", axum::routing::get(handle_socket_upgrade))
        .with_state(state);

    firefly_server::server::serve(&config.api, router).await
}
