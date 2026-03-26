# Login

The login page authenticates users with Nostr via extension, remote signer (NIP-46), or key material, then redirects to `/relays`.

```pug
page(path="/login")
  hero
    badge Secure Nostr Login
    title Welcome back
    description Connect your Nostr account to manage relay hosting, billing, and access in one place.
    bullets
      item Own your identity with cryptographic sign-in.
      item No passwords or email required.
      item Get fast access to your relays.

  auth_card
    section(id="method_select")
      title Log in / Sign up
      description Use any Nostr signer method. New users are automatically onboarded.
      tabs
        tab(id="nip07") Extension
        tab(id="nip46") Signer
        tab(id="key") Key

    when(tab="nip07")
      button Continue with extension

    when(tab="nip46")
      button Continue with signer

    when(tab="key")
      button Continue with key

    screen(id="nip46")
      back_button Back
      title Log in with signer
      tabs
        tab(id="qr") Use QR Code
        tab(id="paste") Paste Link
      when(tab="qr")
        qr_code
        copyable_input(name="nostrconnect_uri")
      when(tab="paste")
        input(name="bunker_url", placeholder="bunker://...")
        button Scan QR code
        button Connect to Signer

    screen(id="key")
      back_button Back
      title Log in with key
      tabs
        tab(id="plaintext") Plaintext
        tab(id="encrypted") Encrypted
      when(tab="plaintext")
        input(name="nsec", placeholder="nsec1...")
      when(tab="encrypted")
        input(name="ncryptsec", placeholder="ncryptsec1...")
        input(type="password", name="password", placeholder="Password")
      button Log in

    error_message

  help_text Having trouble? Make sure your signer is unlocked and connected.
  modal(id="scanner")
    title Scan QR Code
    camera_preview
```
