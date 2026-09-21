use token_service::{AppState, Settings, router};

#[tokio::main]
async fn main() {
    let settings = match Settings::from_env() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let bind_addr = settings.bind_addr.clone();
    let state = match AppState::from_settings(&settings) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("failed to build HTTP client: {err}");
            std::process::exit(1);
        }
    };
    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("cannot listen on {bind_addr}: {err}");
            std::process::exit(1);
        }
    };
    println!("token-service listening on http://{bind_addr}");
    if let Err(err) = axum::serve(listener, router(state)).await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
