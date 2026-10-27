//! Paid memberships: members paying a relay's tenant, through Stripe Connect, for
//! access to the relay.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::relays::{MemberInterval, RelayProfile};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum MembershipStatus {
    /// A checkout was opened and hasn't been paid.
    Pending,
    /// Paid for, and renewing.
    Active,
    /// Paid for until its period ends, and not renewing.
    Canceled,
    /// Over. A lapsed membership is history; paying again opens a new one.
    Lapsed,
}

impl MembershipStatus {
    /// Whether the membership is paid for.
    pub fn is_paid(self) -> bool {
        matches!(self, MembershipStatus::Active | MembershipStatus::Canceled)
    }
}

/// One member's paid access to one relay. A membership bought without a pubkey
/// is an invite: its code is registered as a claim once paid, and the membership
/// goes to the first pubkey without one of its own that joins with it.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Membership {
    pub id: String,
    pub relay_id: String,
    /// The member, or `None` for an invite nobody has used yet.
    pub pubkey: Option<String>,
    /// The code of a membership bought as an invite.
    pub invite_code: Option<String>,
    /// The Stripe customer the membership pays as, once a checkout opens.
    pub stripe_customer_id: Option<String>,
    /// The subscription paying for it, once it is paid.
    pub stripe_subscription_id: Option<String>,
    pub status: MembershipStatus,
    /// When the period paid for ends.
    pub current_period_end: Option<i64>,
    /// When an invite's membership was bound to the pubkey that joined with it.
    pub bound_at: Option<i64>,
    pub created_at: i64,
}

impl Membership {
    /// The customer and subscription a paid membership pays through.
    pub fn subscription(&self) -> Result<(&str, &str)> {
        match (&self.stripe_customer_id, &self.stripe_subscription_id) {
            (Some(customer_id), Some(subscription_id)) => Ok((customer_id, subscription_id)),
            _ => Err(Error::internal("a paid membership has no subscription")),
        }
    }
}

/// What anyone may see of a relay that sells memberships.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    #[serde(flatten)]
    pub relay: RelayProfile,
    /// In cents per `interval`.
    pub price: i64,
    pub interval: MemberInterval,
    /// Whether the tenant can take payments now.
    pub accepting_payment: bool,
}

/// What an invite's buyer sees: its relay, whether it is paid for and bound to
/// whoever joined with it, and the link that accepts it while it is paid for and
/// unbound.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteView {
    pub offer: Offer,
    pub status: MembershipStatus,
    pub bound: bool,
    pub invite_url: Option<String>,
}
