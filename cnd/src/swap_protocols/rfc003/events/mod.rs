// This is fine because we're using associated types
// see: https://github.com/rust-lang/rust/issues/21903
#![allow(type_alias_bounds)]

mod ledger_event_futures;

pub use self::ledger_event_futures::*;

use crate::swap_protocols::{
    asset::Asset,
    rfc003::{
        self,
        ledger::Ledger,
        messages::{AcceptResponseBody, DeclineResponseBody},
        state_machine::HtlcParams,
        Secret,
    },
};
use serde::{Deserialize, Serialize};
use tokio::{self, prelude::future::Either};

type Future<I> = dyn tokio::prelude::Future<Item = I, Error = rfc003::Error> + Send;

#[allow(type_alias_bounds)]
pub type ResponseFuture<AL, BL> = Future<Result<AcceptResponseBody<AL, BL>, DeclineResponseBody>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Funded<L: Ledger, A: Asset> {
    pub transaction: L::Transaction,
    pub asset: A,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Redeemed<L: Ledger> {
    pub transaction: L::Transaction,
    pub secret: Secret,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deployed<L: Ledger> {
    pub transaction: L::Transaction,
    pub location: L::HtlcLocation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refunded<L: Ledger> {
    pub transaction: L::Transaction,
}

impl<L: Ledger> Refunded<L> {
    pub fn new(transaction: L::Transaction) -> Self {
        Self { transaction }
    }
}

pub type DeployedFuture<L: Ledger> = Future<Deployed<L>>;
pub type FundedFuture<L: Ledger, A: Asset> = Future<Funded<L, A>>;
pub type RedeemedOrRefundedFuture<L: Ledger> = Future<Either<Redeemed<L>, Refunded<L>>>;

pub trait LedgerEvents<L: Ledger, A: Asset>: Send {
    fn htlc_deployed(&mut self, htlc_params: HtlcParams<L, A>) -> &mut DeployedFuture<L>;

    fn htlc_funded(
        &mut self,
        htlc_params: HtlcParams<L, A>,
        htlc_deployment: &Deployed<L>,
    ) -> &mut FundedFuture<L, A>;

    fn htlc_redeemed_or_refunded(
        &mut self,
        htlc_params: HtlcParams<L, A>,
        htlc_deployment: &Deployed<L>,
        htlc_funding: &Funded<L, A>,
    ) -> &mut RedeemedOrRefundedFuture<L>;
}

pub trait CommunicationEvents<AL: Ledger, BL: Ledger, AA: Asset, BA: Asset>: Send {
    fn request_responded(
        &mut self,
        request: &rfc003::messages::Request<AL, BL, AA, BA>,
    ) -> &mut ResponseFuture<AL, BL>;
}

pub trait HtlcEvents<L: Ledger, A: Asset>: Send + Sync + 'static {
    fn htlc_deployed(&self, htlc_params: HtlcParams<L, A>) -> Box<DeployedFuture<L>>;
    fn htlc_funded(
        &self,
        htlc_params: HtlcParams<L, A>,
        htlc_deployment: &Deployed<L>,
    ) -> Box<FundedFuture<L, A>>;
    fn htlc_redeemed_or_refunded(
        &self,
        htlc_params: HtlcParams<L, A>,
        htlc_deployment: &Deployed<L>,
        htlc_funding: &Funded<L, A>,
    ) -> Box<RedeemedOrRefundedFuture<L>>;
}
