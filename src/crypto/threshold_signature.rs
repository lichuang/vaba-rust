use crate::base::NodeId;
use crate::base::Value;
use anyhow::Error;
use anyhow::Result;
use std::collections::BTreeMap;

use threshold_crypto::{
    PublicKeySet, PublicKeyShare, SecretKeySet, SecretKeyShare, Signature, SignatureShare,
};

pub struct ThresholdSignatureScheme {
    pk_set: PublicKeySet,

    parties: BTreeMap<NodeId, Party>,
}

pub struct Party {
    sk_share: SecretKeyShare,
    pk_share: PublicKeyShare,
}

impl ThresholdSignatureScheme {
    pub fn new(threshold: usize, node_ids: &[NodeId]) -> Self {
        assert!(node_ids.len() >= threshold);

        let mut rng = rand::thread_rng();
        let sk_set = SecretKeySet::random(threshold, &mut rng);
        let pk_set = sk_set.public_keys();

        let mut parties = BTreeMap::new();
        for id in node_ids {
            let id = *id;
            let sk_share = sk_set.secret_key_share(id);
            let pk_share = pk_set.public_key_share(id);
            let party = Party::new(sk_share, pk_share);
            parties.insert(id as NodeId, party);
        }

        Self { pk_set, parties }
    }

    fn get_party(&self, i: NodeId) -> Result<&Party> {
        self.parties
            .get(&i)
            .ok_or(Error::msg(format!("Cannot find {:?} party in cluster", i)))
    }

    // given party index i and message, return a signature-share of i
    pub fn share_sign(&self, i: NodeId, msg: &Value) -> Result<SignatureShare> {
        Ok(self.get_party(i)?.sign(msg))
    }

    // validate a share sign of message by node i
    pub fn share_validate(&self, i: NodeId, msg: &Value, share_sign: &SignatureShare) -> bool {
        let party = self.parties.get(&i);
        if let Some(party) = party {
            party.validate(msg, share_sign)
        } else {
            false
        }
    }

    // return signature base on share sign sets
    pub fn threshold_sign(&self, share_signs: &[(NodeId, SignatureShare)]) -> Result<Signature> {
        let sign_filter = share_signs.iter().filter_map(|(id, sign)| {
            if self.parties.contains_key(id) {
                Some((*id, sign))
            } else {
                None
            }
        });

        Ok(self
            .pk_set
            .combine_signatures(sign_filter)
            .map_err(|e| anyhow::anyhow!("Threshold crypto combine_signatures error: {:?}", e))?)
    }

    // validate a message signature
    pub fn threshold_validate(&self, msg: &Value, sign: &Signature) -> bool {
        self.pk_set.public_key().verify(sign, msg)
    }
}

impl Party {
    pub fn new(sk_share: SecretKeyShare, pk_share: PublicKeyShare) -> Self {
        Self { sk_share, pk_share }
    }

    pub fn sign(&self, msg: &Value) -> SignatureShare {
        self.sk_share.sign(msg)
    }

    pub fn validate(&self, msg: &Value, sign: &SignatureShare) -> bool {
        self.pk_share.verify(sign, msg.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use threshold_crypto::SignatureShare;

    use super::{NodeId, ThresholdSignatureScheme};

    #[test]
    fn test_threshold_signature_scheme() -> Result<()> {
        let threshold = 3;
        let node_ids = vec![0, 1, 2, 3, 4];
        let threshold_signature = ThresholdSignatureScheme::new(threshold, &node_ids);
        let message = "hello world".to_string();

        // generate share signs
        let mut signature_shares: Vec<(NodeId, SignatureShare)> = Vec::new();
        for id in node_ids {
            signature_shares.push((id, threshold_signature.share_sign(id, &message)?));
        }

        for (i, share_sign) in &signature_shares {
            assert!(threshold_signature.share_validate(*i as NodeId, &message, share_sign));
        }

        let selected_shares: Vec<(NodeId, SignatureShare)> = signature_shares
            .iter()
            .take(threshold + 1)
            .cloned()
            .collect();

        let sign = threshold_signature.threshold_sign(&selected_shares)?;
        assert!(threshold_signature.threshold_validate(&message, &sign));

        Ok(())
    }
}
