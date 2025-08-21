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