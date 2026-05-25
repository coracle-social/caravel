use anyhow::{Result, anyhow};
use nwc::prelude::{
    LookupInvoiceRequest, MakeInvoiceRequest, NWC, NostrWalletConnectURI, PayInvoiceRequest,
    TransactionState,
};

#[derive(Clone)]
pub struct Wallet {
    url: NostrWalletConnectURI,
}

impl Wallet {
    pub fn from_url(url: &str) -> Result<Self> {
        Ok(Self { url: url.parse::<NostrWalletConnectURI>()? })
    }

    pub async fn make_invoice(
        &self,
        amount_msats: u64,
        description: &str,
        expiry_secs: u64,
    ) -> Result<String> {
        let nwc = NWC::new(self.url.clone());
        let result = nwc
            .make_invoice(MakeInvoiceRequest {
                amount: amount_msats,
                description: Some(description.to_string()),
                description_hash: None,
                expiry: Some(expiry_secs),
            })
            .await;
        nwc.shutdown().await;
        Ok(result
            .map_err(|e| anyhow!("failed to create invoice: {e}"))?
            .invoice)
    }

    pub async fn pay_invoice(&self, bolt11: String) -> Result<()> {
        let nwc = NWC::new(self.url.clone());
        let result = nwc.pay_invoice(PayInvoiceRequest::new(bolt11)).await;
        nwc.shutdown().await;
        result.map(|_| ()).map_err(|e| anyhow!("{e}"))
    }

    pub async fn is_settled(&self, bolt11: &str) -> Result<bool> {
        let nwc = NWC::new(self.url.clone());
        let result = nwc
            .lookup_invoice(LookupInvoiceRequest {
                payment_hash: None,
                invoice: Some(bolt11.to_string()),
            })
            .await;
        nwc.shutdown().await;
        let response = result.map_err(|e| anyhow!("failed to lookup invoice: {e}"))?;
        Ok(response.state == Some(TransactionState::Settled) || response.settled_at.is_some())
    }
}
