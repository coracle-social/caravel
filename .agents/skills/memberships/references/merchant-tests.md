# merchant test inventory

Every test in merchant's suite, and the caravel test that covers the same behavior or why none does.

## tests/members.rs

- `signing_in_trades_a_fresh_signature_for_a_token` — `crates/api/tests/auth.rs` a_session_signature_must_be_for_this_request_and_fresh
- `a_member_token_stands_for_its_pubkey_until_it_expires` — `crates/api/tests/auth.rs` a_session_expires_after_thirty_days
- `only_caravel_pushes_relays_and_organizers` — Obsolete: relays and tenants are caravel's own rows; prices are validated by `crates/core/tests/memberships.rs` a_member_price_must_be_chargeable_and_need_a_claim
- `a_relay_page_shows_a_paid_relay_and_whether_it_can_take_payment` — `crates/core/tests/memberships.rs` an_offer_says_whether_payment_is_taken
- `an_invite_offers_its_link_only_while_paid_for_and_unused` — `crates/core/tests/memberships.rs` an_invite_goes_to_whoever_joins_with_it_first
- `an_unused_paid_invite_holds_a_seat` — `crates/core/tests/memberships.rs` an_unused_paid_invite_holds_a_seat
- `memberships_are_released_once_their_relay_stops_charging` — `crates/core/tests/memberships.rs` memberships_are_released_when_their_relay_stops_charging
