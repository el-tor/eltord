
use rand::Rng;
use sha2::{Digest, Sha256};
use hex;

// Generate random payment hash and preimage
pub fn get_random_payhash_and_preimage() -> (String, String) {
    let mut rng = rand::thread_rng();
    let preimage: [u8; 32] = rng.gen();
    let mut hasher = Sha256::new();
    hasher.update(&preimage);
    let payment_hash = hasher.finalize();
    (hex::encode(payment_hash), hex::encode(preimage))
}