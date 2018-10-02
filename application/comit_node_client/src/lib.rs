#![warn(unused_extern_crates, missing_debug_implementations)]
#![deny(unsafe_code)]

extern crate bitcoin_rpc_client;
extern crate common_types;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate structopt;
extern crate bitcoin_support;
extern crate ethereum_support;
extern crate uuid;

pub mod api_client;
pub mod offer;
pub mod order;
pub mod redeem;