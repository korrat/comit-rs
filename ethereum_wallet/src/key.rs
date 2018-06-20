use secp256k1::{PublicKey, Secp256k1, SecretKey};
use tiny_keccak;
use web3::types::Address;

pub trait ToEthereumAddress {
    fn to_ethereum_address(&self) -> Address;
}

impl ToEthereumAddress for PublicKey {
    fn to_ethereum_address(&self) -> Address {
        let serialized = self.serialize_uncompressed();
        // Remove the silly openssl 0x04 byte from the front of the
        // serialized public key. This is a bitcoin thing that
        // ethereum doesn't want. Eth pubkey should be 32 + 32 = 64 bytes.
        let hash = tiny_keccak::keccak256(&serialized[1..]);
        let mut address = Address::default();
        address.copy_from_slice(&hash[12..]);
        address
    }
}

impl ToEthereumAddress for SecretKey {
    fn to_ethereum_address(&self) -> Address {
        let secp = Secp256k1::new();
        PublicKey::from_secret_key(&secp, self)
            .unwrap()
            .to_ethereum_address()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    extern crate hex;
    use std::str::FromStr;

    fn valid_pair(key: &str, address: &str) -> bool {
        let privkey_data = self::hex::decode(key).unwrap();
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&secp, &privkey_data[..]).unwrap();
        let generated_address: Address = secret_key.to_ethereum_address();
        Address::from_str(address).unwrap() == generated_address
    }
    #[test]
    fn test_known_valid_pairs() {
        assert!(valid_pair(
            "981679905857953c9a21e1807aab1b897a395ea0c5c96b32794ccb999a3cd781",
            "5fe3062B24033113fbf52b2b75882890D7d8CA54"
        ));
        assert!(valid_pair(
            "dd0d193e94ad1deb5a45214645ac3793b4f1283d74f354c7849225a43e9cadc5",
            "33DcA4Dfe91388307CF415537D659Fef2d13B71a"
        ));
    }
}
