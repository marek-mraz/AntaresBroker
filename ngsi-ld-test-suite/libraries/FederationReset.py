"""Self-contained federation/IOP reset for the ETSI NGSI-LD test suite.

Resets every broker in a multi-broker (IOP / DistributedOperations) deployment to a clean slate
using ONLY standard NGSI-LD API calls (no DB access). Deleting via the API also clears the broker's
in-VM registry/subscription caches, so no broker restart is needed.

Wired from IOP_TP/__init__.robot as Suite Setup/Teardown, so the suite cleans itself with no external
scripts — keeping the ETSI suite compact and reusable.

Exposed keyword:  Reset Federation Brokers    ${b1_url}    ${b2_url}    ...
(each URL is a broker base, e.g. http://host:9090/ngsi-ld/v1)
"""
import requests

# The test-suite context defines the IOP entity types, so a `type` filter expands to the same IRI the
# entities were stored under. Listing entities/registrations requires a filter (id alone is not
# allowed), so we sweep the known IOP types.
_CONTEXT = (
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/"
    "resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld"
)
_LINK = {"Link": f'<{_CONTEXT}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"'}
_TYPES = ["OffStreetParking", "Vehicle", "Building"]
_TIMEOUT = 10


def _delete_subscriptions(base):
    try:
        r = requests.get(f"{base}/subscriptions", params={"limit": 1000}, timeout=_TIMEOUT)
        for s in (r.json() if r.ok and isinstance(r.json(), list) else []):
            requests.delete(f"{base}/subscriptions/{s['id']}", timeout=_TIMEOUT)
    except Exception:
        pass


def _delete_registrations(base):
    for t in _TYPES:
        try:
            r = requests.get(f"{base}/csourceRegistrations", params={"type": t, "limit": 1000},
                             headers=_LINK, timeout=_TIMEOUT)
            for reg in (r.json() if r.ok and isinstance(r.json(), list) else []):
                requests.delete(f"{base}/csourceRegistrations/{reg['id']}", timeout=_TIMEOUT)
        except Exception:
            pass


def _delete_entities(base):
    for t in _TYPES:
        try:
            r = requests.get(f"{base}/entities", params={"type": t, "limit": 1000, "local": "true"},
                             headers=_LINK, timeout=_TIMEOUT)
            ids = [e["id"] for e in (r.json() if r.ok and isinstance(r.json(), list) else [])]
            if ids:
                requests.post(f"{base}/entityOperations/delete", json=ids,
                              headers={"Content-Type": "application/json"}, timeout=_TIMEOUT)
        except Exception:
            pass


def reset_federation_brokers(*broker_urls):
    """Delete all subscriptions, Context Source Registrations and entities (of the IOP types) on each
    given broker via standard NGSI-LD API calls. Best-effort and idempotent."""
    seen = []
    for url in broker_urls:
        base = url.rstrip("/")
        if not base or base in seen:
            continue
        seen.append(base)
        _delete_subscriptions(base)
        _delete_registrations(base)
        _delete_entities(base)
