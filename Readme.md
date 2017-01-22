# uhttp_request -- HTTP request start-line and header field parsers

[Documentation](https://docs.rs/uhttp_request)

This crate provides barebone, slice-based parsers for extracting [request
line](https://tools.ietf.org/html/rfc7230#section-3.1.1) components and [header
fields](https://tools.ietf.org/html/rfc7230#section-3.2) from HTTP requests.

In general, components are extracted along defined delimiters, but further processing
and syntax validation is left to higher layers.

## Example

```rust
use uhttp_request::{RequestLine, Headers};

let req = b"GET /abc?k=v HTTP/1.1\r\nHost: example.com\r\nAccept: text/*\r\n\r\nbody";

let (reqline, rest) = RequestLine::new(req).unwrap();
assert_eq!(reqline.method, "GET");
assert_eq!(reqline.target, "/abc?k=v");
assert_eq!(reqline.version, "HTTP/1.1");

let mut headers = Headers::new(rest);

let h = headers.next().unwrap().unwrap();
assert_eq!(h.name, "Host");
assert_eq!(h.val, b" example.com");

let h = headers.next().unwrap().unwrap();
assert_eq!(h.name, "Accept");
assert_eq!(h.val, b" text/*");

assert!(headers.next().is_none());

let rest = headers.into_inner();
assert_eq!(rest, b"body");
```
