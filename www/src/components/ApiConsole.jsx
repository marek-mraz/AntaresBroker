import React, { useEffect, useRef } from "react";
import SwaggerUIBundle from "swagger-ui-dist/swagger-ui-bundle.js";
import "swagger-ui-dist/swagger-ui.css";

// The ETSI GS CIM 009 OpenAPI spec (vendored, public/openapi/), rendered by
// Swagger UI. "Execute" issues a normal fetch — the requestInterceptor pins
// the spec's templated server to this origin's /ngsi-ld/v1, which
// virtualhost.js routes into the in-tab wasm broker, so every request also
// lands in the 🛰 log. The broker implements CIM 009 V1.9.1; 1.8.1 is the
// newest published OpenAPI companion (1.9-only surfaces like /entityMaps
// and /snapshots are absent from the docs, not from the broker).
// RapiDoc was tried first and hard-loops on this spec's recursive schemas;
// Models are kept collapsed (-1) for the same reason.
export default function ApiConsole({ onClose }) {
  const ref = useRef(null);
  useEffect(() => {
    const node = ref.current;
    SwaggerUIBundle({
      domNode: node,
      url: new URL("openapi/ngsi-ld-api.yaml", document.baseURI).href,
      docExpansion: "list",
      defaultModelsExpandDepth: -1,
      tryItOutEnabled: true,
      validatorUrl: null,
      requestInterceptor: (req) => {
        req.url = req.url.replace(
          /^https?:\/\/[^/]+\/ngsi-ld\/v1/,
          `${location.origin}/ngsi-ld/v1`,
        );
        return req;
      },
    });
    return () => { node.innerHTML = ""; };
  }, []);
  return (
    <div className="api-console" data-testid="api-console">
      <div className="api-console-bar">
        <strong>📖 NGSI-LD API console</strong>
        <span className="sub">
          ETSI CIM 009 OpenAPI 1.8.1 — “Execute” runs against the wasm broker
          in this tab; requests appear in the 🛰 log
        </span>
        <span className="grow" />
        <button onClick={onClose} data-testid="api-close">✕ close</button>
      </div>
      <div className="api-console-body" ref={ref} />
    </div>
  );
}
