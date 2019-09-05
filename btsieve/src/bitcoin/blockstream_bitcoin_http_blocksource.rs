use crate::blocksource::{self, BlockSource};
use bitcoin_support::{
    deserialize, Block, BlockHeader, FromHex, MinedBlock, Network, OutPoint, Sha256dHash,
    Transaction, TxIn, TxOut,
};
use futures::{Future, Stream};
use reqwest::r#async::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::timer::Interval;

#[derive(Debug)]
pub enum Error {
    UnsupportedNetwork(String),
    Reqwest(reqwest::Error),
    Hex(hex::FromHexError),
    // TODO: Remove
    BlockDeserialization(String),
    WitnessDeserialization(bitcoin_support::consensus::encode::Error),
}

impl From<bitcoin_support::bitcoin_hashes::Error> for Error {
    fn from(e: bitcoin_support::bitcoin_hashes::Error) -> Self {
        Error::BlockDeserialization(format!("Unable to deserialize hash {:?}", e))
    }
}

impl From<hex::FromHexError> for Error {
    fn from(e: hex::FromHexError) -> Self {
        Error::Hex(e)
    }
}

impl From<bitcoin_support::consensus::encode::Error> for Error {
    fn from(e: bitcoin_support::consensus::encode::Error) -> Self {
        Error::WitnessDeserialization(e)
    }
}

/// Blockstream Block as defined by API
/// API: https://github.com/Blockstream/esplora/blob/master/API.md
/// Example: https://blockstream.info/testnet/api/block/00000000142bafadcc937a62e8192c946397860f73523312ba3156d1f73aad85
#[derive(Deserialize)]
pub struct BlockstreamBlock {
    id: String,
    height: u32,
    version: u32,
    timestamp: u32,
    tx_count: u32,
    merkle_root: String,
    previousblockhash: String,
    nonce: u32,
    bits: u32,
}

impl BlockstreamBlock {
    pub fn into_block(self, txs: Vec<Transaction>) -> Result<Block, Error> {
        let Self {
            id,
            height,
            version,
            timestamp,
            tx_count,
            merkle_root,
            previousblockhash,
            nonce,
            bits,
        } = self;

        Ok(Block {
            header: BlockHeader {
                version,
                prev_blockhash: Sha256dHash::from_hex(previousblockhash.as_str())?,
                merkle_root: Sha256dHash::from_hex(merkle_root.as_str())?,
                time: timestamp,
                bits,
                nonce,
            },
            txdata: txs,
        })
    }
}

/// Blockstream Transaction as specified by API
/// API: https://github.com/Blockstream/esplora/blob/master/API.md
/// Example: https://blockstream.info/testnet/api/block/0000000000000179b7f749c03de68e6d8a44be8bd7ec554fc7c3603e00d693da/txs
pub struct BlockstreamTransaction {
    version: u32,
    locktime: u32,
    vin: Vec<BlockstreamTransactionVin>,
    vout: Vec<BlockstreamtransactionVout>,
}

impl BlockstreamTransaction {
    pub fn into_transaction(self) -> Result<Transaction, Error> {
        let Self {
            version,
            locktime,
            vin,
            vout,
        } = self;

        let vin: Result<Vec<TxIn>, Error> = vin.into_iter().map(|vin| vin.into_tx_in()).collect();

        let vout: Result<Vec<TxOut>, Error> =
            vout.into_iter().map(|vout| vout.into_tx_out()).collect();

        Ok(Transaction {
            version,
            lock_time: locktime,
            input: vin?,
            output: vout?,
        })
    }
}

pub struct BlockstreamTransactionVin {
    txid: String,
    vout: u32,
    scriptsig: String,
    sequence: u32,
    witness: Option<Vec<String>>,
}

impl BlockstreamTransactionVin {
    pub fn into_tx_in(self) -> Result<TxIn, Error> {
        let Self {
            txid,
            vout,
            scriptsig,
            sequence,
            witness,
        } = self;

        let witness = match witness {
            Some(witness) => {
                // TODO: to be tested if the witness serialization works like this
                // TODO: We might have to use bitcoin_support::deserialize instead
                let witness: Result<Vec<Vec<u8>>, hex::FromHexError> = witness
                    .into_iter()
                    .map(|witness_stack_row| hex::decode(witness_stack_row))
                    .collect();

                witness?
            }
            None => Vec::new(),
        };

        Ok(TxIn {
            previous_output: OutPoint {
                txid: Sha256dHash::from_hex(txid.as_str())?,
                vout,
            },
            script_sig: deserialize(scriptsig.as_ref())?,
            sequence,
            witness,
        })
    }
}

pub struct BlockstreamtransactionVout {
    scriptpubkey: String,
    value: u64,
}

impl BlockstreamtransactionVout {
    fn into_tx_out(self) -> Result<TxOut, Error> {
        let Self {
            value,
            scriptpubkey,
        } = self;

        Ok(TxOut {
            value,
            script_pubkey: deserialize(scriptpubkey.as_ref())?,
        })
    }
}

#[derive(Clone)]
pub struct BlockstreamBitcoinHttpBlockSource {
    base_url: String,
    client: Client,
}

impl BlockstreamBitcoinHttpBlockSource {
    pub fn new(network: Network) -> Result<Self, Error> {
        match network {
            Network::Mainnet => Ok(Self {
                base_url: "https://blockstream.info/api".to_owned(),
                client: Client::new(),
            }),
            Network::Testnet => Ok(Self {
                base_url: "https://blockstream.info/testnet/api".to_owned(),
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
        // TODO: 1. Get latest 10 blocks using https://github.com/Blockstream/esplora/blob/master/API.md#get-blocksstart_height
        //      Only the last block is needed, but there is no endpoint to retrieve the last block as such (including height and hash)
        //      Thus, just map the array on the fly to just retrieve the block with highest height
        //          use vec: https://stackoverflow.com/questions/44610594/how-can-i-deserialize-json-with-a-top-level-array-using-serde
        //          or max visitor: https://serde.rs/stream-array.html
        // TODO: 2. Fetch all block transactions in bulk (25 per request) using https://github.com/Blockstream/esplora/blob/master/API.md#get-blockhashtxsstart_index
        //      Goes in steps of 25 (i.e. /, /25, /50, ...). tx_count of block data (1.) can be used to stop.
        //      There has to be mechanism to distribute the requests over time (e.g. 1 per sec). Blockstream does not have SLA atm.
        // TODO: 3. Construct MinedBlock from block data retrieved in 1. and transaction data retrieved in 2.
        //      Conversion already implemented (into_ methods in struct impls above), but not tested yet.

        unimplemented!()
    }
}

impl BlockSource for BlockstreamBitcoinHttpBlockSource {
    type Block = MinedBlock;
    type Error = Error;

    fn blocks(
        &self,
    ) -> Box<dyn Stream<Item = Self::Block, Error = blocksource::Error<Error>> + Send> {
        // TODO: This defines the overall polling interval. To be revised.
        // TODO: Since we need multiple requests to fetch Transactions it is set to 10 mins for now
        let poll_interval = match self.network {
            Network::Mainnet => 600,
            Network::Testnet => 600,
            _ => 0,
        };

        log::info!(target: "bitcoin::blocksource", "polling for new blocks from blockstream.info on {} every {} seconds", self.network, poll_interval);

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
                            }
                            Error::Hex(e) => {
                                log::warn!(target: "bitcoin::blocksource", "hex-decode error encountered during polling: {:?}", e);
                                Ok(None)
                            }
                            Error::BlockDeserialization(e) => {
                                log::warn!(target: "bitcoin::blocksource", "block-deserialization error encountered during polling: {:?}", e);
                                Ok(None)
                            }
                            _ => Err(error)
                        }
                    })
                    .map_err(blocksource::Error::Source)
            }).filter_map(|maybe_block| maybe_block);

        Box::new(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_block() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let block_source = BlockstreamBitcoinHttpBlockSource::new(Network::Testnet).unwrap();

        let future = block_source
            .latest_block()
            .map(|block| {
                //                println!(
                //                    "height: {}, block.header.version: {}",
                //                    block.height, block.block.header.version
                //                );
                assert_eq!(592_092, block.height);
            })
            .map_err(|e| panic!(e));

        runtime.block_on(future);
    }
}
