use axum_reverse_proxy::{ReverseProxy};
use axum::{
    body::Body,
    extract::{Request, State},
    http::uri::Uri,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper::StatusCode;
use hyper_util::{rt::TokioExecutor};
use rustls::{ClientConfig, ClientConnection, RootCertStore, pki_types::ServerName};
use std::sync::Arc;
use hyper_rustls::HttpsConnector;
use tower_http::trace::{TraceLayer, DefaultMakeSpan, DefaultOnResponse};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;


type ProxyClient = Client<HttpsConnector<HttpConnector>, Body>;


#[tokio::main]
async fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let config = axum_reverse_proxy::create_dangerous_rustls_config(); 

    let mut http_connector = HttpConnector::new();
    http_connector.enforce_http(false);

    let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1() // Enable HTTP/1.1 fallback
        //.enable_http2() // Enable HTTP/2 for upstream connections
        .wrap_connector(http_connector);

    let client: ProxyClient = Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(https_connector);
    
    let shared_client = Arc::new(client.clone());

    let proxy = ReverseProxy::new_with_client(
        "/",
        "https://192.168.0.1",
        client.clone()
    );

    let app: Router = Router::new()
        .merge(proxy)
        //.route("/{*path}", get(proxy))
        .with_state(client)
        .layer(
            TraceLayer::new_for_http()
            .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
            );

    tokio::spawn(server());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    let _ = axum::serve(listener, app).await;
}

async fn server() {
    let app = Router::new().route("/", get(|| async { "Hello, world!" }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    let _ = axum::serve(listener, app).await;
}

async fn proxy_handler(
    State(client): State<Arc<ProxyClient>>,
    //Path(path): Path<String>,
    mut req: Request<Body>,
    ) -> Result<Response<hyper::body::Incoming>, StatusCode> {
    let upstream_uri = "https://192.168.0.1";
    
    let path = req.uri().path();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(path);
        
    let new_uri = format!("{}{}", upstream_uri, path_and_query);
    *req.uri_mut() = new_uri.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    req.headers_mut().insert(
        hyper::http::header::HOST,
        "192.168.0.1".parse().unwrap(),
    );
    
    println!("{:?}", &req);
    let response = client.request(req).await.map_err(|err| {
        eprintln!("Upstream proxy error: {:?}", err);
        StatusCode::BAD_GATEWAY
    })?;
    println!("{:?}", &response);

    Ok(response)
}
