use std::collections::BTreeMap;

use threshold_crypto::PublicKeySet;
use threshold_crypto::SecretKeySet;
use threshold_crypto::SignatureShare;
use threshold_crypto::SIG_SIZE;

use crate::base::Error;
use crate::base::Message;
use crate::base::NodeId;
use crate::base::Result;
use crate::crypto::Party;

pub type CoinShare = SignatureShare;

pub struct ThresholdCoinTossing {
    pk_set: PublicKeySet,

    parties: BTreeMap<NodeId, Party>,
}

impl ThresholdCoinTossing {
    pub fn new(threshold: usize, total: usize) -> Self {
        assert!(total >= threshold);

        let mut rng = rand::thread_rng();
        let sk_set = SecretKeySet::random(threshold, &mut rng);
        let pk_set = sk_set.public_keys();

        let mut parties = BTreeMap::new();
        for id in 0..total {
            let sk_share = sk_set.secret_key_share(id);
            let pk_share = pk_set.public_key_share(id);
            let party = Party::new(sk_share, pk_share);
            parties.insert(id as NodeId, party);
        }

        Self { pk_set, parties }
    }

    fn get_party(&self, i: NodeId) -> Result<&Party> {
        self.parties.get(&i).ok_or(Error::cluster_error(format!(
            "Cannot find {:?} party in cluster",
            i
        )))
    }

    // return party i's coin share of message
    pub fn coin_share(&self, i: NodeId, msg: &Message) -> Result<CoinShare> {
        Ok(self.get_party(i)?.sign(msg))
    }

    // validate the party i's coin share of message
    pub fn coin_share_validate(&self, i: NodeId, msg: &Message, share: &CoinShare) -> bool {
        let party = self.parties.get(&i);
        if let Some(party) = party {
            party.validate(msg, share)
        } else {
            false
        }
    }

    // given coin share set, return id of selected party
    pub fn coin_toss(&self, msg: &Message, shares: &[(NodeId, CoinShare)]) -> Result<NodeId> {
        let sign_filter = shares.iter().filter_map(|(id, sign)| {
            if let Some(node) = self.parties.get(id) {
                if node.validate(msg, sign) {
                    Some((*id, sign))
                } else {
                    None
                }
            } else {
                None
            }
        });

        let f: [u8; SIG_SIZE] = self
            .pk_set
            .combine_signatures(sign_filter)
            .map_err(Error::crypto_error("fail to combine_signatures"))?
            .to_bytes();

        let s: u64 = f.iter().map(|&x| x as u64).sum();
        let node_ids: Vec<NodeId> = self.parties.keys().cloned().collect();

        Ok(node_ids[s as usize % node_ids.len()])
    }
}

#[cfg(test)]
mod tests {

    use super::{CoinShare, ThresholdCoinTossing};
    use crate::base::{NodeId, Result};

    #[test]
    fn test_threshold_coin_tossing() -> Result<()> {
        let threshold = 3;
        let total = 5;
        let threshold_coin_tossing = ThresholdCoinTossing::new(threshold, total);
        let message = "hello world".to_string();

        // generate share signs
        let signature_shares: Vec<(NodeId, CoinShare)> = (0..total)
            .map(|i| {
                (
                    i as NodeId,
                    threshold_coin_tossing
                        .coin_share(i as NodeId, &message)
                        .unwrap(),
                )
            })
            .collect();

        for (i, share_sign) in &signature_shares {
            assert!(threshold_coin_tossing.coin_share_validate(*i, &message, share_sign))
        }

        // select different threshold + 1 share sign set, it will generate the same node id
        let mut select_share_signs = Vec::new();
        let mut signs = Vec::new();
        for i in 0..total {
            let share = threshold_coin_tossing.coin_share(i as NodeId, &message)?;
            signs.push((i as NodeId, share.clone()));
        }
        for i in 0..total {
            let mut temp_sign = Vec::new();
            for j in 0..total {
                if i == j {
                    continue;
                }
                temp_sign.push(signs[j].clone());
            }
            println!("signs: {:?}", temp_sign);
            select_share_signs.push(temp_sign);
        }
        let mut node_ids = Vec::new();
        for share_signs in &select_share_signs {
            let node_id = threshold_coin_tossing.coin_toss(&message, share_signs);
            node_ids.push(node_id);
        }
        let first = node_ids.first().unwrap();
        assert!(node_ids.iter().all(|x| x == first));

        Ok(())
    }
}
