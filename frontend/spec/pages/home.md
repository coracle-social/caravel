# Home

The home page is a marketing page with CTAs to create a relay or log in (if the user isn't logged in). If the user is logged in, "Sign in" actions are replaced with "Dashboard" and route to `/relays`.

```pug
page(path="/")
  nav(sticky=true)
    brand(name="Caravel", logo="/caravel.png")
    if(authenticated)
      link(href="/relays") Dashboard
    else
      link(href="/login") Sign in

  section(id="hero")
    badge Nostr-native relay hosting
    hero
      title Your community, your relay.
      description Spin up private, managed infrastructure for your community in minutes. Full control over membership, access, and policies — no DevOps required.
    actions
      button(variant="primary", href="/relays/new") Get started free
      if(authenticated)
        button(variant="neutral", href="/relays") Dashboard
      else
        button(variant="neutral", href="/login") Sign in

  section(id="features")
    heading
      title Everything you need
      description Caravel takes care of the infrastructure so you can focus on building your community.
    grid(columns=3)
      card(icon="hosting")
        title Managed hosting
        description We handle uptime, backups, and updates. Your relay stays online so your community never misses a beat.
      card(icon="membership")
        title Membership control
        description Approve members with Nostr pubkeys. Keep your space invite-only or open — you decide.
      card(icon="shield")
        title Access policies
        description Fine-grained write and read permissions. Moderate content without touching any server config.
      card(icon="storage")
        title Blossom storage
        description Attach media files to your relay with integrated Blossom server support on paid plans.
      card(icon="video")
        title LiveKit video
        description Built-in video room support via LiveKit. Host voice and video calls directly within your community.
      card(icon="lightning")
        title Pay with sats
        description Lightning-native billing. No credit cards, no bank accounts — just sats, straight from your wallet.

  section(id="connect")
    heading
      title Connect with your community
      description Once your relay is live, these Nostr-native platforms let your members connect, chat, and collaborate — all powered by your relay.
    grid(columns=2)
      product(name="Flotilla", href="https://flotilla.social", domain="flotilla.social")
        description A community platform built on Nostr. Flotilla gives your members channels, threads, and a rich social experience — all connected to your relay.
        bullets
          item Nostr-native channels & threads
          item Works directly with your relay
          item Open source & self-sovereign
      product(name="Chachi", href="https://chachi.chat", domain="chachi.chat")
        description A group chat app built on top of Nostr. Chachi makes it easy for your community to have real-time conversations, all flowing through your own relay.
        bullets
          item Real-time group messaging
          item Bring your own relay
          item No accounts — just your Nostr key

  section(id="pricing")
    heading
      title Simple pricing
      description Pay in sats. Upgrade or cancel any time.
    pricing_table(cta_href="/relays/new")

  section(id="cta")
    heading
      title Ready to launch your relay?
      description Join communities already running on Caravel. Set up in minutes, pay in sats.
    button(variant="primary", href="/relays/new") Create your relay

  footer
    brand(name="Caravel", logo="/caravel.png")
    copyright © {currentYear} Caravel. Built on Nostr.
    links
      link(href="https://flotilla.social") Flotilla
      link(href="https://chachi.chat") Chachi
```
