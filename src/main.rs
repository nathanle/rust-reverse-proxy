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


//type Client = hyper_util::client::legacy::Client<HttpConnector, Body>;
//
type ProxyClient = Client<HttpsConnector<HttpConnector>, Body>;


#[tokio::main]
async fn main() {

    let config = axum_reverse_proxy::create_dangerous_rustls_config(); 

    //let config = Arc::new(config);

    /*
    let server_name = ServerName::try_from("192.168.0.1")
    .expect("invalid server name");
    let mut client = ClientConnection::new(config, server_name)
    .expect("failed to create client connection");
    */

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
    
    let shared_client = Arc::new(client);

    // 4. Set up the Axum router passing the client via state
    let app: Router = Router::new()
        .route("/{*path}", get(proxy_handler))
        .with_state(shared_client);


    //let client = Client::builder(TokioExecutor::new())
    //.pool_idle_timeout(std::time::Duration::from_secs(120))
    //.build(HttpConnector::new());
    /*
    let proxy = ReverseProxy::new_with_client(
        "/",
        "http://192.168.0.5",
        client
    );
    */

    tokio::spawn(server());

    //let app = Router::new().route("/", get(handler)).with_state(client);
    //let app: Router = proxy.into();

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
    // Define your target upsotream URI destination
    //let query = req.uri().query().map(|q| format!("?{}", q)).unwrap_or_default();
    let upstream_uri = "https://192.168.0.1";
    
    // Construct the new target URI from the incoming path and query
    let path = req.uri().path();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(path);
        
    let new_uri = format!("{}{}", upstream_uri, path_and_query);

    // Rewrite the request URI and host header for the upstream server
    *req.uri_mut() = new_uri.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
     
    req.headers_mut().insert(
        hyper::http::header::HOST,
        "192.168.0.1".parse().unwrap(),
    );
    
    println!("{:?}", &req);
   
    // Forward the request using our custom Rustls-backed client
    let response = client.request(req).await.map_err(|err| {
        eprintln!("Upstream proxy error: {:?}", err);
        StatusCode::BAD_GATEWAY
    })?;
    println!("{:?}", &response);

    Ok(response)
}
