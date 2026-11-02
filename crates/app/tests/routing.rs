//! The routing matrix: what each kind of host answers for each kind of request,
//! through the same dispatcher the binary serves.

use std::collections::HashMap;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use caravel_app::dispatch;
use caravel_app::web::{PageConfig, Web};
use caravel_billing::relays::Relay;
use caravel_core::{Core, hosting, testing};
use caravel_relay::{Relays, RelaysConfig};
use coracle_lib::keys::SecretKey;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const PLATFORM: &str = "platform.test";
const UNKNOWN: &str = "unknown.test";
const BLOB: &str = "a3f2c1d4e5b6a7980123456789abcdef0123456789abcdef0123456789abcdef";

fn files() -> HashMap<String, Vec<u8>> {
    let shell = |name: &str| {
        format!("<html><head><title>{name}</title></head><body></body></html>").into_bytes()
    };
    HashMap::from([
        ("tenant.html".to_string(), shell("tenant")),
        ("admin.html".to_string(), shell("admin")),
        ("portal.html".to_string(), shell("portal")),
        ("assets/app.js".to_string(), b"console.log(1)".to_vec()),
        ("caravel.png".to_string(), b"png".to_vec()),
    ])
}

/// The dispatcher the binary serves, over the relays as they stand now.
async fn app(core: &Core, platform_url: &str) -> Router {
    let relays = Relays::load(
        core.clone(),
        RelaysConfig::default(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let config = PageConfig {
        api_url: format!("{platform_url}/api"),
        ..Default::default()
    };
    let web = Web::with_files(config, files());
    let platform = caravel_util::urls::url_host(platform_url);
    dispatch::router(caravel_api::router(core.clone()), relays, web, platform)
}

struct Fixture {
    core: Core,
    app: Router,
    relay: Relay,
    /// The relay's verified custom domain, its primary host.
    custom: String,
    /// A host that redirects to it.
    old: String,
}

async fn fixture() -> Fixture {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;
    let (relay, _) = testing::create_relay(&core).await;
    let tag = &caravel_util::encoding::random_token()[..12];
    let (custom, old) = (format!("club-{tag}.example"), format!("old-{tag}.example"));
    let settings = serde_json::from_value(json!({"custom_domain": custom})).unwrap();
    let relay = hosting::update_relay(&core, relay, settings).await.unwrap();
    caravel_billing::relays::commands::set_custom_domain_verified(
        core.db.pool(),
        &relay.id,
        &custom,
        true,
    )
    .await
    .unwrap();
    caravel_billing::relays::commands::add_host_redirect(
        core.db.pool(),
        &old,
        &relay.id,
        core.clock.now(),
    )
    .await
    .unwrap();
    let relay = caravel_billing::relays::queries::get_relay(core.db.pool(), &relay.id)
        .await
        .unwrap()
        .unwrap();

    let app = app(&core, &format!("https://{PLATFORM}")).await;

    Fixture {
        core,
        app,
        relay,
        custom,
        old,
    }
}

struct Answer {
    status: StatusCode,
    location: Option<String>,
    body: String,
}

impl Answer {
    fn shell(&self) -> Option<String> {
        let start = self.body.find("<title>")? + 7;
        let end = self.body.find("</title>")?;
        Some(self.body[start..end].to_string())
    }

    fn config(&self) -> Value {
        let start = self.body.find(r#"type="application/json">"#).unwrap() + 24;
        let end = self.body[start..].find("</script>").unwrap() + start;
        serde_json::from_str(&self.body[start..end]).unwrap()
    }
}

async fn get(app: &Router, host: &str, path: &str, accept: &str) -> Answer {
    let request = Request::get(path)
        .header(header::HOST, host)
        .header(header::ACCEPT, accept)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .map(|value| value.to_str().unwrap().to_string());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Answer {
        status,
        location,
        body: String::from_utf8_lossy(&bytes).to_string(),
    }
}

const PAGE: &str = "text/html,application/xhtml+xml";
const JSON: &str = "application/json";
const NIP11: &str = "application/nostr+json";

#[tokio::test]
async fn each_host_answers_for_what_it_serves() {
    let f = fixture().await;
    let canonical = f.relay.canonical_host();
    let (app, custom) = (&f.app, f.custom.as_str());

    // The platform host serves the dashboards and their files.
    assert_eq!(
        get(app, PLATFORM, "/", PAGE).await.shell().as_deref(),
        Some("tenant")
    );
    assert_eq!(
        get(app, PLATFORM, "/relays/abc", PAGE)
            .await
            .shell()
            .as_deref(),
        Some("tenant")
    );
    assert_eq!(
        get(app, PLATFORM, "/admin", PAGE).await.shell().as_deref(),
        Some("admin")
    );
    assert_eq!(
        get(app, PLATFORM, "/admin/relays/abc", PAGE)
            .await
            .shell()
            .as_deref(),
        Some("admin")
    );
    assert_eq!(
        get(app, PLATFORM, "/assets/app.js", "*/*").await.body,
        "console.log(1)"
    );
    assert_eq!(
        get(app, PLATFORM, "/assets/missing.js", "*/*").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(get(app, PLATFORM, "/caravel.png", "*/*").await.body, "png");
    let shell = get(app, PLATFORM, "/", PAGE).await;
    assert_eq!(shell.config()["api_url"], format!("https://{PLATFORM}/api"));

    // The platform host serves the API under /api.
    let plans = get(app, PLATFORM, "/api/plans", JSON).await;
    assert_eq!(plans.status, StatusCode::OK);
    assert!(plans.body.contains(r#""code":"ok""#));

    // A relay's primary host serves its portal to browsers and its protocols to clients.
    let portal = get(app, custom, "/", PAGE).await;
    assert_eq!(portal.shell().as_deref(), Some("portal"));
    assert_eq!(portal.config()["relay"]["relay_id"], f.relay.id);
    assert_eq!(portal.config()["relay"]["host"], custom);
    assert_eq!(
        get(app, custom, "/invites/abc", PAGE)
            .await
            .shell()
            .as_deref(),
        Some("portal")
    );
    let info: Value = serde_json::from_str(&get(app, custom, "/", NIP11).await.body).unwrap();
    assert_eq!(info["self"], f.relay.pubkey);
    assert_eq!(
        get(app, custom, "/assets/app.js", "*/*").await.body,
        "console.log(1)"
    );
    // A blob opened in a tab is a blob, not a page: this relay has no media.
    assert_eq!(
        get(app, custom, &format!("/{BLOB}.png"), PAGE).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(app, custom, "/plans", JSON).await.status,
        StatusCode::NOT_FOUND
    );

    // Its other hosts redirect pages to the primary one and serve the protocols as they are.
    for alias in [canonical.as_str(), f.old.as_str()] {
        let page = get(app, alias, "/join?x=1", PAGE).await;
        assert_eq!(page.status, StatusCode::PERMANENT_REDIRECT, "{alias}");
        assert_eq!(page.location, Some(format!("https://{custom}/join?x=1")));
        let info: Value = serde_json::from_str(&get(app, alias, "/", NIP11).await.body).unwrap();
        assert_eq!(info["self"], f.relay.pubkey, "{alias}");
    }

    // An unknown host serves nothing but the health check and the TLS ask.
    assert_eq!(
        get(app, UNKNOWN, "/", PAGE).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(app, UNKNOWN, "/plans", JSON).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(app, UNKNOWN, "/healthz", "*/*").await.status,
        StatusCode::OK
    );
    let ask = get(
        app,
        UNKNOWN,
        &format!("/domains/check?domain={canonical}"),
        "*/*",
    )
    .await;
    assert_eq!(ask.status, StatusCode::OK);
    let ask = get(app, UNKNOWN, "/domains/check?domain=nobody.example", "*/*").await;
    assert_eq!(ask.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_relay_cant_take_over_the_platforms_host() {
    let core = testing::core(&SecretKey::generate().public_key().to_hex()).await;

    // A relay verified on the platform's host, which only the platform answers for.
    let (relay, _) = testing::create_relay(&core).await;
    caravel_billing::relays::commands::set_custom_domain_verified(
        core.db.pool(),
        &relay.id,
        PLATFORM,
        true,
    )
    .await
    .unwrap();

    let app = app(&core, &format!("https://{PLATFORM}")).await;

    assert_eq!(
        get(&app, PLATFORM, "/", PAGE).await.shell().as_deref(),
        Some("tenant")
    );
    let plans = get(&app, PLATFORM, "/api/plans", JSON).await;
    assert_eq!(plans.status, StatusCode::OK);
    assert!(plans.body.contains(r#""code":"ok""#));
}

#[tokio::test]
async fn a_relay_name_cant_break_out_of_the_portal_config() {
    let f = fixture().await;
    let settings =
        serde_json::from_value(json!({"info_name": "</script><script>alert(1)</script>"})).unwrap();
    hosting::update_relay(&f.core, f.relay.clone(), settings)
        .await
        .unwrap();

    // The fixture's relays reload from the bus; a fresh router reads the row now.
    let app = app(&f.core, &format!("https://{PLATFORM}")).await;

    let portal = get(&app, &f.custom, "/", PAGE).await;
    assert!(!portal.body.contains("</script><script>"));
    assert_eq!(
        portal.config()["relay"]["name"],
        "</script><script>alert(1)</script>"
    );
}
