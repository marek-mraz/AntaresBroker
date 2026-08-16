# Browser example

The broker as a web page — no server process at all.

```bash
./serve.sh            # then open http://localhost:8000/
```

The page installs a Service Worker that answers `/ngsi-ld/v1/*` for the
origin; the playground UI creates entities, subscribes, and shows
notifications arriving in-tab. Hosted copy:
<https://antares-ngsi-ld-demo.marek-mraz.com/>.

Want the same artifact behind a real TCP port (curl-able, e.g. for an
edge box)? That is the Node shim:

```bash
node www/node-shim.mjs 9090
curl -s localhost:9090/q/health     # {"status":"UP","store":"memory",...}
```

Persistence: `ANTARES_STORE=file ANTARES_FILE=/data/antares.redb` gives
the shim the same durable redb store as the native `file` mode; in-page,
the playground can persist via OPFS. Structural limits of a browser build
are listed in the docs' wasm chapter.
