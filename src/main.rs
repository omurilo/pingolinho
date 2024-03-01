use async_trait::async_trait;
use prometheus::register_int_counter;

use pingora::{
    lb::{selection::RoundRobin, LoadBalancer},
    proxy::{http_proxy_service, ProxyHttp, Session},
};
use pingora_core::server::Server;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::Result;
use std::{
    env,
    sync::Arc,
    time::{Duration, SystemTime},
};

pub struct LB {
    req_metric: prometheus::IntCounter,
    backends: Arc<LoadBalancer<RoundRobin>>,
}

pub struct MyCtx {
    duration: SystemTime,
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = MyCtx;
    fn new_ctx(&self) -> Self::CTX {
        MyCtx {
            duration: SystemTime::now(),
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        _ctx.duration = SystemTime::now();

        let upstream = self
            .backends
            .select(b"", 256) // hash doesn't matter for round robin
            .unwrap();

        let peer = Box::new(HttpPeer::new(upstream, false, "".to_string()));
        Ok(peer)
    }

    async fn logging(
        &self,
        session: &mut Session,
        _e: Option<&pingora_core::Error>,
        ctx: &mut Self::CTX,
    ) {
        let response_code = session
            .response_written()
            .map_or(0, |resp| resp.status.as_u16());

        let duration = match ctx.duration.elapsed() {
            Ok(elapsed) => elapsed,
            _ => Duration::new(0, 0),
        };

        println!(
            "{} {:?} response code: {response_code}",
            self.request_summary(session, ctx),
            duration
        );

        self.req_metric.inc();
    }
}

fn main() {
    let mut my_server = Server::new(None).unwrap();
    my_server.bootstrap();

    let upstreams: String = env::var("UPSTREAMS").unwrap();

    println!("upstreams is {upstreams}");

    let upstreams = LoadBalancer::try_from_iter(upstreams.to_string().split(",")).unwrap();

    let mut lb = http_proxy_service(
        &my_server.configuration,
        LB {
            backends: Arc::new(upstreams),
            req_metric: register_int_counter!("reg_counter", "Number of requests").unwrap(),
        },
    );
    lb.add_tcp("0.0.0.0:9999");

    my_server.add_service(lb);

    let mut prometheus_service_http =
        pingora_core::services::listening::Service::prometheus_http_service();
    prometheus_service_http.add_tcp("0.0.0.0:6192");
    my_server.add_service(prometheus_service_http);

    my_server.run_forever();
}
