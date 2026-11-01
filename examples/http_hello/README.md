# Independent loopback HTTP example

This small service uses the shared [loopback HTTP library](../../library/README.md#bounded-loopback-http)
without the settings application, file storage or JSON adapters. It runs on compiler
0.76.0. The Tellur source chooses routes, response statuses, request budgets and
supervised recovery; the shared Rust module supplies bounded synchronous transport.

From the repository root:

```sh
./bin/tellurc check examples/http_hello/main.tellur
./bin/tellurc build examples/http_hello/main.tellur --release
./examples/http_hello/build/main -- 8080 10 3000
```

Arguments are **port, maximum accepted connections, idle milliseconds**. Port 0
chooses an available loopback port; use the printed URL. The budget is 1..1,000,000
and idle timeout 1..60,000 ms. In a second terminal:

```sh
curl -i http://127.0.0.1:8080/hello
curl -I http://127.0.0.1:8080/hello
```

`GET /hello` returns the UTF-8 greeting `Hello from Tellur 🙂`; `HEAD /hello`
returns the same headers without a body. Other methods at that path return 405
with `Allow: GET, HEAD`; unknown paths return 404. Host must match the printed
authority exactly. Requests with an Origin header return 403; requests with a body
at `/hello` return 400. Framing, size and timeout outcomes are mapped explicitly
in `exchange`. Response policy is ordinary Tellur code and can be edited there.

Each accepted connection consumes one unit of the budget, including malformed or
disconnected requests. A normal exchange always completes the connection; both
exchange and close failures are logged when both occur. Listener release follows
the loop. `completed` and `idle` have process status 0, listen/accept failure status 1,
and invalid configuration status 2. A per-request HTTP error does not fail the process.

The lexical boundary retains the connection for contingency. A future source fault
inside request work abandons that work, attempts one generic 500 if transmission has
not started, completes the connection and recovers to the supervised loop. The failed
request never becomes an ordinary outcome. The example ships no fault-triggering
route; tests inject invariant failures into a separate source loader to exercise
500/next-request recovery and avoid a second response after transmission.

This is the library's fixed development profile: loopback only, one request per
connection, bounded headers/body/deadlines, and no TLS, authentication, keep-alive or
concurrency. Its [limits and resource contracts](../../library/README.md#bounded-loopback-http)
apply. A blocking host call does not yet suspend a Tellur task or use an executor.
