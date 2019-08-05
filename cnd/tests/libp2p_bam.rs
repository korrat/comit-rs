use bam::json::{OutgoingRequest, Response};
use cnd::{
    btsieve::BtsieveHttpClient,
    config::Settings,
    libp2p_bam::{BamBehaviour, BehaviourOutEvent, PendingIncomingRequest},
    network::{self, Behaviour, DialInformation},
    swap_protocols::{
        self,
        rfc003::state_store::{InMemoryStateStore, StateStore},
        InMemoryMetadataStore, Metadata, SwapId,
    },
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
    any::Any,
    cmp::Eq,
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

    let (alice_swarm, _alice_address, _alice_peer_id) =
        spawn_actor(&mut runtime, ping_headers.clone());
    let (_bob_swarm, bob_address, bob_peer_id) = spawn_actor(&mut runtime, ping_headers);

    let bobs_response = {
        let mut alice_swarm = alice_swarm.lock().expect("should be able to lock swarm");
        let response = alice_swarm.bam.send_request(
            DialInformation {
                peer_id: bob_peer_id,
                address_hint: Some(bob_address),
            },
            OutgoingRequest::new("PING"),
        );

        response
    };

    let bobs_response = runtime.block_on(bobs_response).unwrap();

    assert_eq!(bobs_response, Response::new(bam::Status::OK(0)))
}

#[test]
fn unknown_frame_returns_error() {
    let settings = Settings::from_default();

    let mut metadata: HashMap<&str, Metadata> = HashMap::new();
    let mut states: HashMap<&str, Box<dyn Any + Send + Sync>> = HashMap::new();

    let metadata_store = Arc::new(InMemoryMetadataStore {
        metadata: Mutex::new(metadata),
    });
    let state_store = Arc::new(InMemoryStateStore {
        states: Mutex::new(states),
    });
    let btsieve_client = create_btsieve_api_client(&settings);

    let bob_protocol_dependencies = swap_protocols::bob::ProtocolDependencies {
        ledger_events: btsieve_client.clone().into(),
        metadata_store: Arc::clone(&metadata_store),
        state_store: Arc::clone(&state_store),
        seed: settings.comit.secret_seed,
    };

    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    let behaviour = network::Behaviour::new(bob_protocol_dependencies, runtime.executor()).unwrap();

    let mut ping_headers = HashMap::new();
    ping_headers.insert(String::from("SOME_UNKNOWN_TYPE"), HashSet::new());

    let (alice_swarm, _alice_address, _alice_peer_id) =
        spawn_actor(&mut runtime, ping_headers.clone());
    let (_bob_swarm, bob_address, bob_peer_id) = spawn_actor(&mut runtime, ping_headers);

    let bobs_response = {
        let mut alice_swarm = alice_swarm.lock().expect("should be able to lock swarm");
        let response = alice_swarm.bam.send_request(
            DialInformation {
                peer_id: bob_peer_id,
                address_hint: Some(bob_address),
            },
            OutgoingRequest::new("SOME_UNKNOWN_TYPE"),
        );

        response
    };

    let bobs_response = runtime.block_on(bobs_response).unwrap();

    assert_eq!(bobs_response, Response::new(bam::Status::OK(0)))
}

fn create_btsieve_api_client(settings: &Settings) -> BtsieveHttpClient {
    BtsieveHttpClient::new(
        &settings.btsieve.url,
        settings.btsieve.bitcoin.poll_interval_secs,
        settings.btsieve.bitcoin.network,
        settings.btsieve.ethereum.poll_interval_secs,
        settings.btsieve.ethereum.network,
    )
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
    PeerId,
) {
    let keypair = Keypair::generate_ed25519();
    let peer_id = PeerId::from_public_key(keypair.public());

    let behaviour = TestBehaviour {
        bam: BamBehaviour::new(known_request_headers),
    };
    let transport = MemoryTransport::default()
        .with_upgrade(SecioConfig::new(keypair))
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

    let mut swarm = libp2p::Swarm::new(transport, behaviour, peer_id.clone());
    let listen: Multiaddr = format!("/memory/{}", OsRng.next_u32())
        .parse()
        .expect("should be a valid memory address");

    libp2p::Swarm::listen_on(&mut swarm, listen.clone())
        .expect("swarm failed to listen on memory address");
    let swarm = Arc::new(Mutex::new(swarm));

    {
        let swarm = swarm.clone();
        let swarm_worker = futures::stream::poll_fn(move || {
            swarm.lock().expect("should be able to lock swarm").poll()
        })
        .for_each(|_| Ok(()))
        .map_err(|e| {
            log::error!("failed with {:?}", e);
        });

        runtime.spawn(swarm_worker);
    }

    (swarm, listen, peer_id)
}
