use crate::{auth::Bearer, runtime_client::RuntimeClient, service::CoKernelService};
use http::{Response, StatusCode};
use http_body_util::{BodyExt, Full, Limited, combinators::BoxBody};
use hyper::{body::Bytes, server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use std::{convert::Infallible, io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinHandle,
};
use tower_service::Service;
pub struct McpServer {
    pub address: SocketAddr,
    stop: watch::Sender<bool>,
    task: JoinHandle<io::Result<()>>,
}
fn status(code: StatusCode) -> Response<BoxBody<Bytes, Infallible>> {
    let mut response =
        Response::new(Full::new(Bytes::from_static(b"CoKernel MCP request rejected")).boxed());
    *response.status_mut() = code;
    if code == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            http::header::WWW_AUTHENTICATE,
            http::HeaderValue::from_static("Bearer"),
        );
    }
    response
}
async fn bounded_request(
    request: http::Request<hyper::body::Incoming>,
    hosts: &[String; 2],
) -> Result<http::Request<Full<Bytes>>, StatusCode> {
    let host = request.headers().get_all(http::header::HOST);
    if host.iter().count() != 1
        || !host
            .iter()
            .next()
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| hosts.iter().any(|a| a == h))
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let origins = request.headers().get_all(http::header::ORIGIN);
    if origins.iter().count() > 1
        || origins.iter().next().is_some_and(|o| {
            !o.to_str()
                .is_ok_and(|o| hosts.iter().any(|h| o == format!("http://{h}")))
        })
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let (parts, body) = request.into_parts();
    let bytes = Limited::new(body, 256 * 1024)
        .collect()
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?
        .to_bytes();
    if parts.method == http::Method::POST {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
        if !value.is_object() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if let Some(id) = value.get("id") {
            let valid = match id {
                serde_json::Value::String(s) => {
                    !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
                }
                serde_json::Value::Number(n) => n
                    .as_i64()
                    .is_some_and(|n| n.unsigned_abs() <= 9_007_199_254_740_991),
                _ => false,
            };
            if !valid {
                return Err(StatusCode::BAD_REQUEST);
            }
        }
    }
    Ok(http::Request::from_parts(parts, Full::new(bytes)))
}
impl McpServer {
    pub async fn bind(
        address: SocketAddr,
        bearer: Bearer,
        runtime: RuntimeClient,
    ) -> io::Result<Self> {
        if !address.ip().is_loopback() {
            return Err(io::Error::other("CK-MCP-CONFIG: MCP must bind loopback"));
        }
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        let authority = address.to_string();
        let hosts = Arc::new([authority.clone(), format!("localhost:{}", address.port())]);
        let config = crate::streamable_http_config()
            .with_max_request_body_bytes(256 * 1024)
            .with_allowed_hosts([authority.clone(), format!("localhost:{}", address.port())])
            .with_allowed_origins([
                format!("http://{authority}"),
                format!("http://localhost:{}", address.port()),
            ]);
        let cancel = config.cancellation_token.clone();
        let service = StreamableHttpService::new(
            move || {
                Ok(CoKernelService {
                    runtime: runtime.clone(),
                })
            },
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let bearer = Arc::new(bearer);
        let capacity = Arc::new(Semaphore::new(16));
        let (stop, mut stopping) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _=stopping.changed()=>break,
                    _=connections.join_next(),if !connections.is_empty()=>{},
                    accepted=listener.accept()=>{
                        let (stream,_)=accepted?;
                        let Ok(permit)=capacity.clone().try_acquire_owned() else {continue;};
                        let service=service.clone();let bearer=bearer.clone();let hosts=hosts.clone();
                        connections.spawn(async move {
                            let _permit=permit;
                            let serve=service_fn(move |request: http::Request<hyper::body::Incoming>| {
                                let mut service=service.clone();let bearer=bearer.clone();let hosts=hosts.clone();
                                async move {
                                    let authorization=request.headers().get_all(http::header::AUTHORIZATION);
                                    if authorization.iter().count()!=1 || !bearer.accepts(authorization.iter().next().and_then(|v|v.to_str().ok())) {return Ok::<_,Infallible>(status(StatusCode::UNAUTHORIZED));}
                                    if request.uri().path()!="/mcp" || request.uri().query().is_some() {return Ok(status(StatusCode::NOT_FOUND));}
                                    let request=match bounded_request(request,&hosts).await {Ok(request)=>request,Err(code)=>return Ok(status(code))};
                                    service.call(request).await
                                }
                            });
                            let mut builder=http1::Builder::new();
                            builder.keep_alive(false).max_headers(64).max_buf_size(16*1024).timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(5));
                            let _=tokio::time::timeout(Duration::from_secs(130),builder.serve_connection(TokioIo::new(stream),serve)).await;
                        });
                    }
                }
            }
            cancel.cancel();
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            Ok(())
        });
        Ok(Self {
            address,
            stop,
            task,
        })
    }
    pub async fn shutdown(self) -> io::Result<()> {
        self.stop.send_replace(true);
        self.task.await.map_err(io::Error::other)?
    }
    pub async fn wait(&mut self) -> io::Result<()> {
        (&mut self.task).await.map_err(io::Error::other)?
    }
}
