use bech32::Bech32;
use bech32::primitives::decode::CheckedHrpstring;

/// What a BOLT 11 invoice states about a payment. The invoice's signature isn't
/// checked, so these are only what the invoice claims.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invoice {
    /// The amount in millisatoshis, unless the invoice leaves it to the payer.
    pub msats: Option<u64>,
    pub payment_hash: Option<String>,
    pub description_hash: Option<String>,
    pub created_at: u64,
}

/// The tagged fields this decoder reads, by their bech32 character.
const PAYMENT_HASH: u8 = 1;
const DESCRIPTION_HASH: u8 = 23;

/// The 104 five-bit groups of an invoice's signature, which close its data.
const SIGNATURE_LENGTH: usize = 104;

/// Read an invoice, or `None` if it isn't one.
pub fn decode(invoice: &str) -> Option<Invoice> {
    let checked = CheckedHrpstring::new::<Bech32>(invoice).ok()?;
    let hrp = checked.hrp().to_lowercase();
    let data: Vec<u8> = checked
        .fe32_iter::<std::iter::Empty<u8>>()
        .map(|fe| fe.to_u8())
        .collect();

    let body = data.get(..data.len().checked_sub(SIGNATURE_LENGTH)?)?;
    let created_at = body
        .get(..7)?
        .iter()
        .fold(0u64, |acc, &value| (acc << 5) | u64::from(value));
    let mut invoice = Invoice {
        msats: read_amount(hrp.strip_prefix("ln")?)?,
        payment_hash: None,
        description_hash: None,
        created_at,
    };

    let mut fields = &body[7..];
    while let [tag, high, low, rest @ ..] = fields {
        let length = usize::from(*high) << 5 | usize::from(*low);
        let value = rest.get(..length)?;
        match *tag {
            PAYMENT_HASH if length == 52 => invoice.payment_hash = Some(to_hex(value)),
            DESCRIPTION_HASH if length == 52 => invoice.description_hash = Some(to_hex(value)),
            _ => {}
        }
        fields = &rest[length..];
    }

    Some(invoice)
}

/// The amount after the currency prefix, scaled by its multiplier, in
/// millisatoshis: `Some(None)` when there is no amount, `None` when it can't be read.
fn read_amount(rest: &str) -> Option<Option<u64>> {
    let Some(start) = rest.find(|c: char| c.is_ascii_digit()) else {
        return Some(None);
    };
    let amount = &rest[start..];
    let (digits, multiplier) = match amount.char_indices().last()? {
        (index, c) if c.is_ascii_alphabetic() => (&amount[..index], Some(c)),
        _ => (amount, None),
    };
    let value: u64 = digits.parse().ok()?;
    let msats = match multiplier {
        None => value.checked_mul(100_000_000_000)?,
        Some('m') => value.checked_mul(100_000_000)?,
        Some('u') => value.checked_mul(100_000)?,
        Some('n') => value.checked_mul(100)?,
        Some('p') if value.is_multiple_of(10) => value / 10,
        _ => return None,
    };

    Some(Some(msats))
}

/// Five-bit groups as bytes, dropping the bits left over, as hex.
fn to_hex(groups: &[u8]) -> String {
    let mut bytes = Vec::with_capacity(groups.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0);
    for &group in groups {
        acc = (acc << 5) | u32::from(group);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }

    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The zap receipt invoice from NIP-57's example: 10 uBTC.
    const INVOICE: &str = "lnbc10u1p3unwfusp5t9r3yymhpfqculx78u027lxspgxcr2n2987mx2j55nnfs95nxnzqpp5jmrh92pfld78spqs78v9euf2385t83uvpwk9ldrlvf6ch7tpascqhp5zvkrmemgth3tufcvflmzjzfvjt023nazlhljz2n9hattj4f8jq8qxqyjw5qcqpjrzjqtc4fc44feggv7065fqe5m4ytjarg3repr5j9el35xhmtfexc42yczarjuqqfzqqqqqqqqlgqqqqqqgq9q9qxpqysgq079nkq507a5tw7xgttmj4u990j7wfggtrasah5gd4ywfr2pjcn29383tphp4t48gquelz9z78p4cq7ml3nrrphw5w6eckhjwmhezhnqpy6gyf0";

    #[test]
    fn reads_an_invoice() {
        assert_eq!(
            decode(INVOICE),
            Some(Invoice {
                msats: Some(1_000_000),
                payment_hash: Some(
                    "96c772a829fb7c780410f1d85cf12a89e8b3c78c0bac5fb47f62758bf961ec30".to_string()
                ),
                description_hash: Some(
                    "132c3de7685de2be270c4ff629092c92dea8cfa2fdff212a65bf56b95527900e".to_string()
                ),
                created_at: 1_674_164_540,
            })
        );
    }

    #[test]
    fn scales_amounts_by_their_multiplier() {
        assert_eq!(read_amount("bc"), Some(None));
        assert_eq!(read_amount("bc2m"), Some(Some(200_000_000)));
        assert_eq!(read_amount("tb25n"), Some(Some(2_500)));
        assert_eq!(read_amount("bc10p"), Some(Some(1)));
        assert_eq!(read_amount("bc15p"), None);
        assert_eq!(read_amount("bcrt1"), Some(Some(100_000_000_000)));
    }

    #[test]
    fn refuses_what_isnt_an_invoice() {
        let mut corrupted = INVOICE.to_string();
        corrupted.pop();
        corrupted.push('q');

        assert_eq!(decode("hello"), None);
        assert_eq!(decode(&corrupted), None);
    }
}
