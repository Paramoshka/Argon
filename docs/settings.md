## Argon Ingress Annotations

### Scope: These annotations configure how the proxy connects to your upstream (backend).
> Applies to: The whole Ingress resource. If you need different settings per backend, split into multiple Ingresses.

---

### Annotations
> argon.github.io/backend-protocol

Controls the upstream connection protocol from the proxy to your backend.
Allowed values (case-insensitive):

| Value    | Transport | HTTP version | Notes                                           |
| -------- | --------- | ------------ | ----------------------------------------------- |
| `h1`     | TCP       | HTTP/1.1     | Plain HTTP to backend.                          |
| `h1-ssl` | TLS       | HTTP/1.1     | TLS to backend; ALPN advertises `http/1.1`.     |
| `h2`     | TCP       | HTTP/2       | Cleartext HTTP/2 (h2c). Backend must speak h2c. |
| `h2-ssl` | TLS       | HTTP/2       | TLS + HTTP/2; ALPN advertises `h2`.             |

### Defaults: If omitted or invalid, the controller uses h1.

### Notes

* These settings affect upstream only. The client → proxy side still negotiates HTTP/1.1 or HTTP/2 independently.
For *-ssl, SNI is set from the upstream authority/Host. The backend certificate must match that value.

---
> argon.github.io/backend-timeout

Per-request upstream timeout in milliseconds. This caps the total time to connect (and TLS handshake if applicable) and send the request / receive the response headers from the backend.

Type: integer (recommend quoting in YAML, e.g. "15000").

Default: 3000 (3s).

Valid range: 1–120000 ms (values outside are clamped or rejected; see controller logs).

On timeout, the proxy returns 504 Gateway Timeout to the client.

---
> argon.github.io/backend-retries

Total number of attempts the proxy should make per request when contacting the backend. Includes the initial try; set to `1` to disable retries.

- Type: integer (quote in YAML) — `"1"`, `"2"`, etc.
- Default: `1`
- Allowed range: 1–10 (values outside the range are clamped to the nearest bound)

Retries are only issued when the proxy can safely replay the request (e.g., the body has already been fully read). Timeout and connector failures are retried until the limit is reached.

---
> argon.github.io/lb-algorithm

Controls how Argon distributes requests across endpoints within a backend cluster.

Allowed values (case-insensitive):

| Value          | Description                                                             |
| -------------- | ----------------------------------------------------------------------- |
| `roundrobin`   | Default. Rotates requests evenly across all healthy endpoints.          |
| `leastconn`    | Routes each request to the endpoint with the fewest in-flight requests. |

Default: `roundrobin`.

If the value is missing, misspelled, or unsupported, the controller keeps using round-robin.

---
> argon.github.io/request-headers

Status: ⏳ work in progress — annotation name & schema can still change before GA.

Allows mutating HTTP request headers before the proxy forwards traffic upstream. The value must be YAML (or JSON) that decodes into a list of operations:

```yaml
argon.github.io/request-headers: |
  - name: X-Debug
    value: "1"
    mode: set
  - name: X-Trace
    value: "canary"
    mode: append
  - name: X-Legacy
    mode: remove
```

Supported modes:
- `set` — replace all existing values of the header with the provided `value`.
- `append` — add an extra value alongside existing ones.
- `remove` — delete the header entirely (the `value` field is ignored).

If the annotation is missing or fails to parse, no header rewrites are applied.

---
> argon.github.io/backend-tls-insecure-skip-verify

Controls whether the dataplane verifies the backend's TLS certificate when `backend-protocol` uses TLS (`h1-ssl`/`h2-ssl`).

- Type: boolean (quote in YAML): `"true"` or `"false"`
- Default: `false` (verify certificates against system roots)
- When `true`, the dataplane accepts any backend certificate (self-signed, expired, or mismatched).

Warning: This is insecure. Use only for development or when you explicitly trust the backend.

---
## External Auth (Dex / oauth2-proxy)

These annotations enable external authorization via an oauth2-proxy–compatible endpoint (commonly backed by Dex). When enabled, the dataplane:

- Makes a subrequest to `auth-url` with forwarded headers (`Cookie`, `Authorization`, `X-Forwarded-*`, `X-Original-URI`, `X-Auth-Request-Redirect`).
- If the auth service responds 2xx, copies headers listed in `auth-response-headers` from the auth response into the upstream request.
- If the auth service responds 401/403, issues a 302 redirect to `auth-signin` (template variables supported, see below).
- Skips auth for any request path that starts with any prefix listed in `auth-skip-paths` (use this for `/oauth2/` routes to oauth2-proxy itself).
- Optionally performs a fast “redirect-first” check if `auth-cookie-name` is set and no such cookie is present.

> argon.github.io/auth-url

HTTP URL of the oauth2-proxy auth endpoint (typically `/oauth2/auth`). Must be resolvable from the dataplane (ClusterIP form is recommended).

Example: `http://oauth2-proxy.default.svc.cluster.local:4180/oauth2/auth`

> argon.github.io/auth-signin

Signin URL template for unauthorized clients (typically `/oauth2/start?rd=...`). The following variables are substituted:

- `$host` — original request Host header (without port)
- `$escaped_request_uri` — percent-encoded original path and query
- `$scheme` — `http` or `https` based on the client → proxy connection

Example: `http://$host/oauth2/start?rd=$escaped_request_uri`

> argon.github.io/auth-response-headers

Comma-separated list of header names to copy from the auth response to the upstream request when auth succeeds (2xx).

Example: `X-Auth-Request-User, X-Auth-Request-Email, X-Auth-Request-Preferred-Username`

> argon.github.io/auth-skip-paths

Comma-separated list of path prefixes that bypass auth (e.g., route `/oauth2/` to oauth2-proxy itself to handle callbacks and starts).

Example: `/oauth2/, /healthz`

> argon.github.io/auth-cookie-name

If set, the proxy checks whether the cookie is present on the incoming request. If absent, it redirects to `auth-signin` immediately (before making the subrequest). This speeds up UX but does not validate cookie contents.

Example: `_oauth2_proxy`

### Example (Echo protected by oauth2-proxy)

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: echo-auth
  annotations:
    kubernetes.io/ingress.class: argon
    argon.github.io/auth-url: "http://oauth2-proxy.default.svc.cluster.local:4180/oauth2/auth"
    argon.github.io/auth-signin: "http://$host/oauth2/start?rd=$escaped_request_uri"
    argon.github.io/auth-response-headers: "X-Auth-Request-User, X-Auth-Request-Email"
    argon.github.io/auth-skip-paths: "/oauth2/"
    argon.github.io/auth-cookie-name: "_oauth2_proxy"
spec:
  ingressClassName: argon
  rules:
    - host: echo.local
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: echo
                port:
                  number: 80
```

Notes:
- Ensure `/oauth2/` is routed to the oauth2-proxy Service via a separate path (or separate Ingress) to avoid loops.
- Set Dex issuer to a publicly reachable URL (e.g., `http://dex.local/dex`) and configure oauth2-proxy `--oidc-issuer-url` accordingly.

---
### Examples
HTTP/1.1 over TLS (backend speaks HTTPS), 15s timeout
```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: echo
  annotations:
    argon.github.io/backend-protocol: "h1-ssl"
    argon.github.io/backend-timeout: "15000"
    argon.github.io/backend-tls-insecure-skip-verify: "true"   # optional; accept self-signed certs
spec:
  rules:
  - host: echo.example.com
    http:
      paths:
      - path: /tls
        pathType: Prefix
        backend:
          service:
            name: echo
            port:
              number: 8443

```
