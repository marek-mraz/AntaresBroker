# ETSI test-suite tool bugs (never hack the broker around these)

Log of defects in the ETSI Robot suite itself, per the testing guide: prove
the suite contradicts the spec, record it here, leave the broker correct.
Each entry: TP id, what the suite does, why it is wrong (spec clause), status.

## Known (inherited from the Scorpio campaign in this workspace)

### IOP QueryEntities 04_01 / 04_02 — duplicate-id setup → self-inflicted 409

The suite's own setup creates two payloads with the same entity id on one
broker; the second create correctly gets 409 AlreadyExists (CIM 009 5.6.1),
which the test then reports as a failure. Broker behaviour is spec-correct.
Status: open upstream; excluded from gating conclusions. (See memory
`multi-broker-fed-stack.md`.)

## New entries

### 2026-08-05 — forge.etsi.org serves an incomplete TLS chain (infra, not a TP)

`forge.etsi.org` presents ONLY its leaf certificate (`CN=*.etsi.org`, issuer
`GeoTrust TLS RSA CA G1`) with no intermediate in the handshake. Strict chain
builders — rustls/webpki (the broker), OpenSSL (curl), python `ssl` (the
Robot suite itself) — all fail with "unable to get local issuer certificate";
browsers mask it via AIA chasing. Effect: every `@context` fetch from the
forge and every suite-side resolve fails, turning the whole run red with
`LdContextNotAvailable` (first seen as 33/33 CommonBehaviours failures; runs
on 2026-08-04 were green, so the server changed in between).

Not a broker bug and not hacked around: verification stays on. Fix was a
deliberate trust-anchor addition of the PUBLIC DigiCert intermediate
(`dev/ca-extra.pem`): brokers via `ANTARES_EXTRA_CA_FILE` (compose), the
suite + curl via `REQUESTS_CA_BUNDLE`/`CURL_CA_BUNDLE`/`SSL_CERT_FILE`
(pipeline).

**RESOLVED 2026-08-05, same day**: ETSI fixed their server — `openssl
s_client -connect forge.etsi.org:443 -showcerts` now returns the full
3-cert chain (leaf + GeoTrust TLS RSA CA G1 + DigiCert Global Root G2).
Both wirings and `dev/ca-extra.pem` removed. The `ANTARES_EXTRA_CA_FILE`
knob itself stays in the broker — it is the documented mechanism for
private CAs (§16.4).

### 2026-08-05 — MqttUtils launches mosquitto with no readiness wait (tool bug, fixed in fork)

`Start Mqtt Server` (resources/mqttUtils/MqttUtils.resource) does
`docker rm -f` + `docker run -d` and returns immediately; the test's first
MQTT connect races the mosquitto daemon start. On a cold docker daemon the
race is lost reliably: `058_02_02` fails with
`ConnectionRefusedError: [Errno 111]` while `058_02_01` (image already warm)
passes. Fixed in the fork: `Wait Until Keyword Succeeds` polling the broker
port (15 s / 0.5 s) after the `docker run`, before the keyword returns. Also
load-bearing: the mosquitto container must be the ONLY occupant of
`compose-files_default` (it is addressed by the hardcoded 172.29.9.2 mapping)
— the ETSI compose therefore keeps the db containers on their own `dbs`
network, and the pipeline creates `compose-files_default` for every run since
the compose now references it as external.
