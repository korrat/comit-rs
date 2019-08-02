use bam::json::Response;
use cnd::libp2p_bam::{BamBehaviour, BehaviourOutEvent, PendingIncomingRequest};
use futures::{future::Future, Stream};
use libp2p::{
    core::{
        muxing::{StreamMuxerBox, SubstreamRef},
        swarm::{NetworkBehaviour, NetworkBehaviourEventProcess},
        transport::MemoryTransport,
        upgrade::{InboundUpgradeExt, OutboundUpgradeExt},
    },
    identity::Keypair,
    Multiaddr, NetworkBehaviour, PeerId, Transport,
};
use rand::{rngs::OsRng, RngCore};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::prelude::*;

#[derive(NetworkBehaviour)]
struct TestBehaviour<TSubstream> {
    bam: BamBehaviour<TSubstream>,
}

impl<TSubstream> NetworkBehaviourEventProcess<BehaviourOutEvent> for TestBehaviour<TSubstream> {
    fn inject_event(&mut self, event: BehaviourOutEvent) {
        match event {
            BehaviourOutEvent::PendingIncomingRequest { request, peer_id } => {
                let PendingIncomingRequest { request, channel } = request;

                channel.send(Response::new(bam::Status::OK(0))).unwrap();
            }
        }
    }
}

#[test]
fn ping_responds_with_pong() {
    let request = r#"{"type":"REQUEST","payload":{"type":"PING"}}"#;
    let expected_response = r#"{"type":"RESPONSE","payload":{},"status":"OK00"}"#;

    let mut runtime = tokio::runtime::Runtime::new().unwrap();

    let alice = spawn_actor(&mut runtime);
    let bob = spawn_actor(&mut runtime);
}

fn spawn_actor(
    runtime: &mut tokio::runtime::Runtime,
) -> (
    Arc<
        Mutex<
            libp2p::Swarm<
                libp2p::core::transport::boxed::Boxed<
                    (PeerId, StreamMuxerBox),
                    libp2p::core::either::EitherError<
                        libp2p::core::transport::upgrade::TransportUpgradeError<
                            libp2p::core::transport::memory::MemoryTransportError,
                            libp2p::secio::SecioError,
                        >,
                        libp2p::core::upgrade::UpgradeError<std::io::Error>,
                    >,
                >,
                TestBehaviour<SubstreamRef<Arc<StreamMuxerBox>>>,
            >,
        >,
    >,
    Multiaddr,
    Keypair,
) {
    let keypair = Keypair::generate_ed25519();

    let behaviour = TestBehaviour {
        bam: BamBehaviour::new(HashMap::new()),
    };
    let transport = libp2p::core::transport::memory::MemoryTransport::default()
        .with_upgrade(libp2p::secio::SecioConfig::new(keypair.clone()))
        .and_then(move |output, endpoint| {
            let peer_id = output.remote_key.into_peer_id();
            let peer_id2 = peer_id.clone();
            let upgrade = libp2p::yamux::Config::default()
                .map_inbound(move |muxer| (peer_id, muxer))
                .map_outbound(move |muxer| (peer_id2, muxer));
            libp2p::core::upgrade::apply(output.stream, upgrade, endpoint)
                .map(|(id, muxer)| (id, StreamMuxerBox::new(muxer)))
        })
        .boxed();

    let peer_id = PeerId::from_public_key(keypair.public());

    let mut swarm = libp2p::Swarm::new(transport, behaviour, peer_id);
    let listen: Multiaddr = format!("/memory/{}", OsRng.next_u32()).parse().unwrap();

    libp2p::Swarm::listen_on(&mut swarm, listen.clone())
        .expect("swarm failed to listen on memory address");
    let swarm = Arc::new(Mutex::new(swarm));

    {
        let swarm = swarm.clone();
        let swarm_worker = futures::stream::poll_fn(move || swarm.lock().unwrap().poll())
            .for_each(|_| Ok(()))
            .map_err(|e| {
                log::error!("failed with {:?}", e);
            });

        runtime.spawn(swarm_worker);
    }

    (swarm, listen, keypair)
}
