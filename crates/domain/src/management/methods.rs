//! The NIP-86 methods a relay's managers may grant to other pubkeys.

/// Every NIP-86 method a pubkey may be granted on a relay.
pub const MANAGED_METHODS: [&str; 23] = [
    "changerelayname",
    "changerelaydescription",
    "changerelayicon",
    "banpubkey",
    "unbanpubkey",
    "allowpubkey",
    "unallowpubkey",
    "listbannedpubkeys",
    "listallowedpubkeys",
    "banevent",
    "allowevent",
    "listbannedevents",
    "createrole",
    "editrole",
    "deleterole",
    "assignrole",
    "unassignrole",
    "assignmethod",
    "unassignmethod",
    "listmethodassignees",
    "listclaims",
    "createclaim",
    "deleteclaim",
];

/// The methods every member holds on a relay whose members may invite.
pub const INVITE_METHODS: [&str; 2] = ["listclaims", "createclaim"];
