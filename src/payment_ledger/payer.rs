//! Explicit native payer selection. Both adapters share the same immutable
//! signed invoice type and durable ledger; neither selection changes authority.
use async_trait::async_trait;
use lni::{
    cln::foundation::{
        AdapterError, ClnFoundation, ConstructionPaymentProof, DispatchRequest, InvoiceExpectation,
        NativeAccount, Observation, PaymentStatus, ValidatedInvoice,
    },
    phoenixd::foundation::PhoenixdFoundation,
};
type Result<T> = std::result::Result<T, AdapterError>;

pub enum FoundationPayer {
    Cln(ClnFoundation),
    Phoenixd(PhoenixdFoundation),
}
impl From<ClnFoundation> for FoundationPayer {
    fn from(value: ClnFoundation) -> Self {
        Self::Cln(value)
    }
}
impl From<PhoenixdFoundation> for FoundationPayer {
    fn from(value: PhoenixdFoundation) -> Self {
        Self::Phoenixd(value)
    }
}
impl FoundationPayer {
    pub fn account(&self) -> NativeAccount {
        match self {
            Self::Cln(w) => w.account(),
            Self::Phoenixd(w) => w.account(),
        }
    }
    pub fn restore_for_lookup(
        &self,
        text: &str,
        digest: [u8; 32],
        expected: InvoiceExpectation,
    ) -> Result<ValidatedInvoice> {
        match self {
            Self::Cln(w) => w.restore_for_lookup(text, digest, expected),
            Self::Phoenixd(w) => w.restore_for_lookup(text, digest, expected),
        }
    }
    pub fn restore_bolt12_for_offer_lookup(
        &self,
        text: &str,
        digest: [u8; 32],
        offer: &str,
        id: [u8; 32],
        expected: InvoiceExpectation,
    ) -> Result<ValidatedInvoice> {
        match self {
            Self::Cln(w) => w.restore_bolt12_for_offer_lookup(text, digest, offer, id, expected),
            Self::Phoenixd(w) => {
                w.restore_bolt12_for_offer_lookup(text, digest, offer, id, expected)
            }
        }
    }
    pub async fn fetch_bolt12(
        &self,
        offer: &str,
        id: [u8; 32],
        expected: InvoiceExpectation,
        observation: Observation,
    ) -> Result<ValidatedInvoice> {
        match self {
            Self::Cln(w) => w.fetch_bolt12(offer, id, expected, observation).await,
            Self::Phoenixd(w) => w.fetch_bolt12(offer, id, expected, observation).await,
        }
    }
    pub async fn lookup_construction_proof(
        &self,
        invoice: &ValidatedInvoice,
        operation: [u8; 16],
        observation: Observation,
    ) -> Result<Option<ConstructionPaymentProof>> {
        match self {
            Self::Cln(w) => {
                w.lookup_construction_proof(invoice, operation, observation)
                    .await
            }
            Self::Phoenixd(w) => {
                w.lookup_construction_proof(invoice, operation, observation)
                    .await
            }
        }
    }
    pub async fn dispatch(
        &self,
        invoice: &ValidatedInvoice,
        request: DispatchRequest,
        observation: Observation,
    ) -> Result<PaymentStatus> {
        match self {
            Self::Cln(w) => w.dispatch(invoice, request, observation).await,
            Self::Phoenixd(w) => w.dispatch(invoice, request, observation).await,
        }
    }
    pub async fn lookup(
        &self,
        invoice: &ValidatedInvoice,
        operation: [u8; 16],
        observation: Observation,
    ) -> Result<PaymentStatus> {
        match self {
            Self::Cln(w) => w.lookup(invoice, operation, observation).await,
            Self::Phoenixd(w) => w.lookup(invoice, operation, observation).await,
        }
    }
}
#[async_trait]
pub trait PayerDriver: Send + Sync {
    fn account(&self) -> NativeAccount;
    async fn dispatch(
        &self,
        invoice: &ValidatedInvoice,
        request: DispatchRequest,
        observation: Observation,
    ) -> Result<PaymentStatus>;
    async fn lookup(
        &self,
        invoice: &ValidatedInvoice,
        operation: [u8; 16],
        observation: Observation,
    ) -> Result<PaymentStatus>;
}
macro_rules! driver {
    ($t:ty) => {
        #[async_trait]
        impl PayerDriver for $t {
            fn account(&self) -> NativeAccount {
                <$t>::account(self)
            }
            async fn dispatch(
                &self,
                i: &ValidatedInvoice,
                r: DispatchRequest,
                o: Observation,
            ) -> Result<PaymentStatus> {
                <$t>::dispatch(self, i, r, o).await
            }
            async fn lookup(
                &self,
                i: &ValidatedInvoice,
                r: [u8; 16],
                o: Observation,
            ) -> Result<PaymentStatus> {
                <$t>::lookup(self, i, r, o).await
            }
        }
    };
}
driver!(ClnFoundation);
driver!(PhoenixdFoundation);
driver!(FoundationPayer);
#[async_trait]
impl<W: PayerDriver + ?Sized> PayerDriver for std::sync::Arc<W> {
    fn account(&self) -> NativeAccount {
        self.as_ref().account()
    }
    async fn dispatch(
        &self,
        i: &ValidatedInvoice,
        r: DispatchRequest,
        o: Observation,
    ) -> Result<PaymentStatus> {
        self.as_ref().dispatch(i, r, o).await
    }
    async fn lookup(
        &self,
        i: &ValidatedInvoice,
        r: [u8; 16],
        o: Observation,
    ) -> Result<PaymentStatus> {
        self.as_ref().lookup(i, r, o).await
    }
}
