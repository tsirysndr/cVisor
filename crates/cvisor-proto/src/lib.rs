//! Generated gRPC types + client/server stubs for the cVisor daemon contract.
//!
//! `cvisor_proto::cvisor` holds the prost messages, the `cvisor_client::CvisorClient`
//! (used by the CLI's remote mode), and the `cvisor_server::Cvisor` trait
//! (implemented by the daemon).

pub mod cvisor {
    tonic::include_proto!("cvisor");
}

/// The encoded proto file descriptor set, for gRPC server reflection.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("cvisor_descriptor");
