use bam::json::{OutgoingRequest, Response};
use cnd::{
    libp2p_bam::{BamBehaviour, BehaviourOutEvent, PendingIncomingRequest},
    network::DialInformation,
};
use futures::{future::Future, Stream};
use libp2p::{
    core::{
        either::EitherError,
        muxing::{StreamMuxerBox, SubstreamRef},
        swarm::NetworkBehaviourEventProcess,
        transport::{
            boxed::Boxed, memory::MemoryTransportError, upgrade::TransportUpgradeError,
            MemoryTransport,
        },
        upgrade::{InboundUpgradeExt, OutboundUpgradeExt, UpgradeError},
    },
    identity::Keypair,
    secio::{SecioConfig, SecioError},
    Multiaddr, NetworkBehaviour, PeerId, Transport,
};
use rand::{rngs::OsRng, RngCore};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

#[derive(NetworkBehaviour)]
struct TestBehaviour<TSubstream> {
    bam: BamBehaviour<TSubstream>,
}

impl<TSubstream> NetworkBehaviourEventProcess<BehaviourOutEvent> for TestBehaviour<TSubstream> {
    fn inject_event(&mut self, event: BehaviourOutEvent) {
        match event {
            BehaviourOutEvent::PendingIncomingRequest { request, .. } => {
                let PendingIncomingRequest { channel, .. } = request;

                channel.send(Response::new(bam::Status::OK(0))).unwrap();
            }
        }
    }
}

#[test]
fn ping_responds_with_ok() {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();

    let mut ping_headers = HashMap::new();
    ping_headers.insert(String::from("PING"), HashSet::new());

    let (alice_swarm, _alice_address, _alice_key) = spawn_actor(&mut runtime, HashMap::new());
    let (_bob_swarm, bob_address, bob_key) = spawn_actor(&mut runtime, ping_headers);

    let response = {
        let mut alice_swarm = alice_swarm.lock().unwrap();
        let response = alice_swarm.bam.send_request(
            DialInformation {
                peer_id: PeerId::from_public_key(bob_key.public()),
                address_hint: Some(bob_address),
            },
            OutgoingRequest::new("PING"),
        );

        response
    };

    let response = runtime.block_on(response).unwrap();

    assert_eq!(response, Response::new(bam::Status::OK(0)))
}

fn spawn_actor(
    runtime: &mut tokio::runtime::Runtime,
    known_request_headers: HashMap<String, HashSet<String>>,
) -> (
    Arc<
        Mutex<
            libp2p::Swarm<
                Boxed<
                    (PeerId, StreamMuxerBox),
                    EitherError<
                        TransportUpgradeError<MemoryTransportError, SecioError>,
                        UpgradeError<std::io::Error>,
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
        bam: BamBehaviour::new(known_request_headers),
    };
    let transport = MemoryTransport::default()
        .with_upgrade(SecioConfig::new(keypair.clone()))
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
