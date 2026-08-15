//! Wires the two front-ends. gRPC (tonic) runs on a multi-threaded tokio
//! runtime; the actix GraphQL server runs on its own actix runtime (own thread).
//! Both are guarded by the same bearer token.

use std::net::SocketAddr;
use std::sync::Arc;

use actix_web::{web, App, HttpServer};
use tonic::transport::Server;

use cvisor_proto::cvisor::cvisor_server::CvisorServer;

use crate::auth::Auth;
use crate::graphql;
use crate::grpc::Grpc;
use crate::registry::Registry;

/// Serve gRPC on `addr`, rejecting requests without a valid bearer token in the
/// `authorization` metadata.
// The interceptor must return tonic's `Result<_, Status>`; Status is large.
#[allow(clippy::result_large_err)]
pub async fn run_grpc(
    addr: SocketAddr,
    reg: Arc<Registry>,
    auth: Arc<Auth>,
) -> Result<(), Box<dyn std::error::Error>> {
    let svc = CvisorServer::with_interceptor(Grpc::new(reg), move |req: tonic::Request<()>| {
        let header = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        if auth.check_header(header) {
            Ok(req)
        } else {
            Err(tonic::Status::unauthenticated("invalid or missing token"))
        }
    });

    // Server reflection is served without the auth interceptor, so tools like
    // grpcurl can discover the API anonymously (calling Cvisor RPCs still needs
    // the token). Register both the v1 and v1alpha reflection services.
    let reflect_v1 = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(cvisor_proto::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let reflect_v1alpha = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(cvisor_proto::FILE_DESCRIPTOR_SET)
        .build_v1alpha()?;

    Server::builder()
        .add_service(reflect_v1)
        .add_service(reflect_v1alpha)
        .add_service(svc)
        .serve(addr)
        .await?;
    Ok(())
}

/// Serve GraphQL (queries/mutations at `/graphql`, subscriptions at
/// `/graphql/ws`, GraphiQL on GET `/graphql`) on `addr`. Runs its own actix
/// runtime, so call it on a dedicated thread.
pub fn run_http(addr: SocketAddr, reg: Arc<Registry>, auth: Arc<Auth>) -> std::io::Result<()> {
    actix_web::rt::System::new().block_on(async move {
        let schema = graphql::schema(reg);
        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(schema.clone()))
                .app_data(web::Data::new(auth.clone()))
                .route("/graphql", web::post().to(graphql::graphql_handler))
                .route("/graphql", web::get().to(graphql::graphql_playground))
                .route("/graphql/ws", web::get().to(graphql::graphql_ws))
        })
        .bind(addr)?
        .run()
        .await
    })
}
