#![warn(rust_2018_idioms)]
#![forbid(unsafe_code)]

pub mod bitcoin;
pub mod ethereum;

#[cfg(test)]
pub mod quickcheck;

use futures::{Future, Stream};

pub trait MatchingTransactions<Q>: Send + Sync + 'static {
    type Transaction;

    fn matching_transactions(
        &self,
        query: Q,
    ) -> Box<dyn Stream<Item = Self::Transaction, Error = ()> + Send>;
}

pub trait LatestBlock: Send + Sync + 'static {
    type Error: std::fmt::Debug;
    type Block;
    type BlockHash;

    fn latest_block(
        &self,
    ) -> Box<dyn Future<Item = Self::Block, Error = Self::Error> + Send + 'static>;
}

pub trait BlockByHash: Send + Sync + 'static {
    type Error: std::fmt::Debug;
    type Block;
    type BlockHash;

    fn block_by_hash(
        &self,
        block_hash: Self::BlockHash,
    ) -> Box<dyn Future<Item = Self::Block, Error = Self::Error> + Send + 'static>;
}

pub trait ReceiptByHash: Send + Sync + 'static {
    type Receipt;
    type TransactionHash;
    type Error: std::fmt::Debug;

    fn receipt_by_hash(
        &self,
        transaction_hash: Self::TransactionHash,
    ) -> Box<dyn Future<Item = Self::Receipt, Error = Self::Error> + Send + 'static>;
}
