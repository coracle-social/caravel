Nostr integration layer for the frontend. It initializes shared Nostr primitives (event store, relay pool, account manager), wires signer transport, persists local account state, and exposes reactive helpers for auth/account/profile data.

## Constants

### `API_URL`

- Reads `VITE_API_URL` from environment.

### `PLATFORM_NAME`

- Reads `VITE_PLATFORM_NAME` from environment.
- Falls back to `"Caravel"`.

### Storage keys

- `caravel.accounts`: serialized account list.
- `caravel.activeAccount`: currently selected account id.

## Shared singletons

### `eventStore`

- Global `EventStore` used to cache and observe Nostr events (profiles, relay lists, etc).

### `pool`

- Global `RelayPool` used for relay connections, subscriptions, and publishing.

### `accounts`

- Global `AccountManager` that holds all known local accounts and active account state.

## Startup wiring

- Registers common account types with `registerCommonAccountTypes(accounts)`.
- Configures event loading via `createEventLoaderForStore(eventStore, pool, ...)` with lookup and extra relay seeds.
- Binds `NostrConnectSigner` network methods to the app relay pool:
  - `subscriptionMethod = pool.subscription.bind(pool)`
  - `publishMethod = pool.publish.bind(pool)`

## `function restoreAccounts()`

- Restores serialized accounts from localStorage.
- Ignores corrupted local data safely (catch-and-continue).
- Restores the active account id if it still exists in the restored account set.

## `function activateAccount(account: IAccount)`

- Adds the account to `accounts`.
- Sets it as active.
- Persists account state to localStorage via `persistAccounts()`.

## `function persistAccounts()`

- Serializes all accounts into `caravel.accounts`.
- Persists active account id to `caravel.activeAccount`.
- Removes `caravel.activeAccount` if no active account exists.

## `function useActiveAccount()`

- Solid helper that returns a reactive signal for current active account.
- Subscribes to `accounts.active$` and cleans up the subscription automatically.

## `function useProfilePicture(pubkey: () => string | undefined)`

- Reactive helper to resolve a user profile picture URL from a pubkey.
- If pubkey is missing, clears picture to `undefined`.
- Subscribes to profile updates in `eventStore` and maps profile to a picture via `getProfilePicture`.
- Calls `primeProfiles([pubkey])` to proactively fetch profile data.
- Cleans up both profile and network subscriptions when dependencies change/unmount.

## `function primeProfiles(pubkeys: string[])`

- Preloads Nostr profile events (`kind: 0`) for a set of pubkeys.
- Deduplicates and filters invalid pubkeys.
- Early-returns a no-op unsubscribable when no pubkeys are provided.
- Uses currently connected relays as seeds (`pool.relays.keys()`).
- Optionally fetches relay list events (`kind: 10002`) from seed relays to improve mailbox/outbox routing.
- Builds optimized relay routing by:
  - including mailbox pointers from event store,
  - applying fallback relays when needed,
  - selecting optimal relays with connection limits,
  - converting to an outbox map.
- Opens an outbox subscription for profile events and stores incoming events in `eventStore` (ignoring `EOSE`).
- Returns an object with `unsubscribe()` that tears down both profile and mailbox-seed subscriptions.
