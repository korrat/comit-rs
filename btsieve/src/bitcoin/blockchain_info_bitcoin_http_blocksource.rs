use crate::blocksource::{self, BlockSource};
use bitcoin_support::{
    deserialize, Block, BlockHeader, FromHex, MinedBlock, Network, OutPoint, Script, Sha256dHash,
    Transaction, TxIn, TxOut,
};
use futures::{Future, Stream};
use reqwest::r#async::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::timer::Interval;

#[derive(Deserialize)]
struct BlockchainInfoLatestBlock {
    hash: String,
    height: u32,
}

#[derive(Debug)]
pub enum Error {
    UnsupportedNetwork(String),
    Reqwest(reqwest::Error),
    Hex(hex::FromHexError), // TODO: Remove
    BlockDeserialization(String),
}

impl From<bitcoin_support::bitcoin_hashes::Error> for Error {
    fn from(e: bitcoin_support::bitcoin_hashes::Error) -> Self {
        Error::BlockDeserialization(format!("Unable to deserialize hash"))
    }
}

#[derive(Clone)]
pub struct BlockchainInfoHttpBlockSource {
    network: Network,
    client: Client,
}

#[derive(Deserialize)]
struct BlockchainInfoRawBlock {
    ver: u32,
    prev_block: String,
    mrkl_root: String,
    time: u32,
    bits: u32,
    nonce: u32,
    tx: Vec<BlockchainInfoRawBlockTransaction>,
}

impl BlockchainInfoRawBlock {
    pub fn into_block(self) -> Result<Block, Error> {
        let Self {
            ver,
            prev_block,
            mrkl_root,
            time,
            bits,
            nonce,
            tx,
        } = self;

        let txs = tx
            .into_iter()
            .map(|raw_block_tx| raw_block_tx.into_tx())
            .collect::<Vec<_>>();

        Ok(Block {
            header: BlockHeader {
                version: ver,
                prev_blockhash: Sha256dHash::from_hex(prev_block.as_str())?,
                merkle_root: Sha256dHash::from_hex(mrkl_root.as_str())?,
                time,
                bits,
                nonce,
            },
            txdata: txs,
        })
    }
}

#[derive(Deserialize)]
struct BlockchainInfoRawBlockTransaction {
    lock_time: u32,
    ver: u32,
    inputs: Vec<BlockchainInfoRawBlockTransactionInput>,
    out: Vec<BlockchainInfoRawBlockTransactionOutput>,
}

impl BlockchainInfoRawBlockTransaction {
    pub fn into_tx(self) -> Transaction {
        let Self {
            lock_time,
            ver,
            inputs,
            out,
        } = self;

        Transaction {
            version: ver,
            lock_time,
            input: inputs
                .into_iter()
                .map(|raw_block_tx_in| raw_block_tx_in.into_tx_in())
                .collect::<Vec<_>>(),
            output: out
                .into_iter()
                .map(|raw_block_tx_out| raw_block_tx_out.into_tx_out())
                .collect::<Vec<_>>(),
        }
    }
}

#[derive(Deserialize)]
struct BlockchainInfoRawBlockTransactionInput {
    sequence: u32,
    witness: Vec<u8>,
    prev_out: Option<BlockchainInfoRawBlockTransactionOutput>,
    script: Vec<u8>,
}

impl BlockchainInfoRawBlockTransactionInput {
    pub fn into_tx_in(self) -> TxIn {
        let Self {
            sequence,
            witness,
            prev_out,
            script,
        } = self;

        TxIn {
            previous_output: prev_out
                .map(|prev_out| OutPoint::default())
                .unwrap_or_else(|| OutPoint::null()), // TODO: match properly
            script_sig: Script::from(hex::decode(script).unwrap()), // TODO: fix unwrap
            sequence,
            witness: unimplemented!(), // TODO: map properly, matching problems
        }
    }
}

#[derive(Deserialize)]
struct BlockchainInfoRawBlockTransactionOutput {
    //    addr_tag: Option<String>,
    //    spent: bool,
    //    spending_outpoints: Vec<BlockchainInfoRawBlockTransactionOutputSpendingOutpoint>,
    //    tx_index: u32,
    //    #[serde(alias = "type")]
    //    tx_out_type: u32,
    //    addr: String,
    value: u64,
    //    n: u32,
    script: String,
}

impl BlockchainInfoRawBlockTransactionOutput {
    fn into_tx_out(self) -> TxOut {
        let Self { value, script } = self;

        TxOut {
            value,
            script_pubkey: Script::from(hex::decode(script).unwrap()), // TODO: fix unwrap,
        }
    }
}

#[derive(Deserialize)]
struct BlockchainInfoRawBlockTransactionOutputSpendingOutpoint {
    tx_index: u32,
    n: u32,
}

impl BlockchainInfoHttpBlockSource {
    pub fn new(network: Network) -> Result<Self, Error> {
        // Currently configured for Testnet

        match network {
            Network::Testnet => Ok(Self {
                network,
                client: Client::new(),
            }),
            _ => {
                log::error!(
                    "Network {} not supported for bitcoin http blocksource",
                    network
                );
                Err(Error::UnsupportedNetwork(format!(
                    "Network {} currently not supported for bitcoin http plocksource",
                    network
                )))
            }
        }
    }

    pub fn latest_block(&self) -> impl Future<Item = MinedBlock, Error = Error> + Send + 'static {
        let cloned_self = self.clone();

        self.latest_block_without_tx()
            .and_then(move |latest_block| {
                cloned_self.raw_hex_block(latest_block.hash, latest_block.height)
            })
    }

    fn base_url(&self) -> String {
        match self.network {
            Network::Testnet => "https://testnet.blockchain.info".to_string(),
            _ => panic!(
                "Network {} not supported for bitcoin.info blocksource",
                self.network
            ),
        }
    }

    fn latest_block_without_tx(
        &self,
    ) -> impl Future<Item = BlockchainInfoLatestBlock, Error = Error> + Send + 'static {
        // https://blockchain.info/q/latesthash only works for mainnet, there is no testnet endpoint
        // we fall-back to [testnet.]blockchain.info/latestblock to retrieve the latest
        // block hash

        let latest_block_url = format!("{}/latestblock", self.base_url());

        self.client
            .get(latest_block_url.as_str())
            .send()
            .map_err(Error::Reqwest)
            .and_then(move |mut response| {
                response
                    .json::<BlockchainInfoLatestBlock>()
                    .map_err(Error::Reqwest)
            })
    }

    fn raw_hex_block(
        &self,
        block_hash: String,
        block_height: u32,
    ) -> impl Future<Item = MinedBlock, Error = Error> + Send + 'static {
        let raw_block_by_hash_url =
            format!("{}/rawblock/{}?format=hex", self.base_url(), block_hash);

        self.client
            .get(raw_block_by_hash_url.as_str())
            .send()
            .map_err(Error::Reqwest)
            .and_then(|mut response| response.text().map_err(Error::Reqwest))
            .and_then(|response_text| hex::decode(response_text).map_err(Error::Hex))
            .and_then(move |bytes| {
                deserialize(bytes.as_ref())
                    .map(|block| {
                        log::trace!("Got {:?}", block);
                        MinedBlock::new(block, block_height)
                    })
                    .map_err(|e| {
                        log::error!("Got new block but failed to deserialize it because {:?}", e);
                        Error::BlockDeserialization(format!(
                            "Failed to deserialize the response from blockchain.info into a block: {}", e
                        ))
                    })
            })
    }
}

impl BlockSource for BlockchainInfoHttpBlockSource {
    type Block = MinedBlock;
    type Error = Error;

    fn blocks(
        &self,
    ) -> Box<dyn Stream<Item = Self::Block, Error = blocksource::Error<Error>> + Send> {
        // https://www.blockchain.com/api/q (= https://www.blockchain.info/api/q) states:
        //  "Please limit your queries to a maximum of 1 every 10 seconds." (29/08/2019)
        //
        // The Bitcoin blockchain has a mining interval of about 10 minutes.
        // The poll interval is configured to once every 5 minutes.
        let poll_interval = match self.network {
            Network::Mainnet => 300,
            _ => 0,
        };

        log::info!(target: "bitcoin::blocksource", "polling for new blocks from blockchain.info on {} every {} seconds", self.network, poll_interval);

        let cloned_self = self.clone();

        let stream = Interval::new_interval(Duration::from_secs(poll_interval))
            .map_err(blocksource::Error::Timer)
            .and_then(move |_| {
                cloned_self
                    .latest_block()
                    .map(Some)
                        .or_else(|error| {
                            match error {
                                Error::Reqwest(e) => {
                                    log::warn!(target: "bitcoin::blocksource", "reqwest error encountered during polling: {:?}", e);
                                    Ok(None)
                                },
                                Error::Hex(e) => {
                                    log::warn!(target: "bitcoin::blocksource", "hex-decode error encountered during polling: {:?}", e);
                                    Ok(None)
                                },
                                Error::BlockDeserialization(e) => {
                                    log::warn!(target: "bitcoin::blocksource", "block-deserialization error encountered during polling: {:?}", e);
                                    Ok(None)
                                },
                                _ => Err(error)
                            }
                        })
                        .map_err(blocksource::Error::Source)
            }).filter_map(|maybe_block| maybe_block);

        Box::new(stream)
    }
}
