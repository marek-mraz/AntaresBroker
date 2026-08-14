//! 5.8.1.4: the broker hands its own URL to peer brokers as the
//! distributed-subscription notification endpoint. When ANTARES_PUBLIC_URL
//! is unset the default must still be reachable — i.e. carry the HTTP port
//! (http://{host_alias} alone points at port 80; ETSI-matrix ADV_02 shape).

use antares_api::AppState;

// one test: env vars are process-global, parallel tests would race
#[test]
fn default_public_url_carries_the_http_port() {
    std::env::remove_var("ANTARES_PUBLIC_URL");
    std::env::set_var("ANTARES_HTTP_PORT", "9091");
    let st = AppState::new("antares2".into());
    assert_eq!(st.public_url, "http://antares2:9091");
    // must NOT be the portless form — that is the unreachable default
    assert_ne!(st.public_url, "http://antares2");

    // explicit ANTARES_PUBLIC_URL always wins
    std::env::set_var("ANTARES_PUBLIC_URL", "http://localhost:9094");
    let st = AppState::new("antares5".into());
    assert_eq!(st.public_url, "http://localhost:9094");
    std::env::remove_var("ANTARES_PUBLIC_URL");

    // port 80 (and unset port) stay portless — canonical http form
    std::env::set_var("ANTARES_HTTP_PORT", "80");
    let st = AppState::new("antares1".into());
    assert_eq!(st.public_url, "http://antares1");
}
