//! This crate provides barebone, slice-based parsers for extracting [request
//! line](https://tools.ietf.org/html/rfc7230#section-3.1.1) components and [header
//! fields](https://tools.ietf.org/html/rfc7230#section-3.2) from HTTP requests.
//!
//! In general, components are extracted along defined delimiters, but further processing
//! and syntax validation is left to higher layers.
//!
//! ## Example
//!
//! ```rust
//! use uhttp_request::{RequestLine, Headers};
//!
//! let req = b"GET /abc?k=v HTTP/1.1\r\nHost: example.com\r\nAccept: text/*\r\n\r\nbody";
//!
//! let (reqline, rest) = RequestLine::new(req).unwrap();
//! assert_eq!(reqline.method, "GET");
//! assert_eq!(reqline.target, "/abc?k=v");
//! assert_eq!(reqline.version, "HTTP/1.1");
//!
//! let mut headers = Headers::new(rest);
//!
//! let h = headers.next().unwrap().unwrap();
//! assert_eq!(h.name, "Host");
//! assert_eq!(h.val, b" example.com");
//!
//! let h = headers.next().unwrap().unwrap();
//! assert_eq!(h.name, "Accept");
//! assert_eq!(h.val, b" text/*");
//!
//! assert!(headers.next().is_none());
//!
//! let rest = headers.into_inner();
//! assert_eq!(rest, b"body");
//! ```

#![feature(field_init_shorthand)]
#![feature(conservative_impl_trait)]

extern crate memchr;

use memchr::memchr;

/// Errors that may occur when processing request header.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Error {
    /// Unexpected EOF.
    Partial,
    /// Malformed syntax.
    Syntax,
}

/// Specialized result using custom `Error`.
pub type Result<T> = std::result::Result<T, Error>;

/// A "Request-Line" [RFC7230§3.1.1] that begins an HTTP request.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct RequestLine<'a> {
    /// Request method on target resource.
    ///
    /// This is guaranteed to be free of spaces but is not guaranteed to be free of other
    /// whitespace or otherwise syntactically correct.
    pub method: &'a str,

    /// Target resource of request.
    ///
    /// This is guaranteed to be free of spaces but is not guaranteed to be free of other
    /// whitespace or otherwise syntactically correct.
    pub target: &'a str,

    /// HTTP protocol version of request.
    ///
    /// This is guaranteed to be free of spaces but is not guaranteed to be free of other
    /// whitespace or otherwise syntactically correct.
    pub version: &'a str,
}

impl<'a> RequestLine<'a> {
    /// Try to parse the given bytes into `RequestLine` components.
    ///
    /// On success, return `Ok((rl, rest))`, where `rl` is the `RequestLine` and `rest` is
    /// a slice that begins directly after the Request-Line terminating CRLF.
    pub fn new(buf: &'a [u8]) -> Result<(Self, &'a [u8])> {
        // Ignore leading empty lines [RFC7230§3.5].
        let start = skip_empty_lines(buf)?;

        // Retrieve contents of initial line and split by spaces.
        let (line, rest) = next_line(start)?;
        let line = std::str::from_utf8(line).map_err(|_| Error::Syntax)?;

        let mut chunks = line.split(' ');
        let method = chunks.next().ok_or(Error::Syntax)?;
        let target = chunks.next().ok_or(Error::Syntax)?;
        let version = chunks.next().ok_or(Error::Syntax)?;

        if chunks.next().is_some() {
            return Err(Error::Syntax);
        }

        Ok((RequestLine { method, target, version }, rest))
    }
}

/// An HTTP request header field [RFC7230§3.2].
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub struct Header<'a> {
    /// Header name, with surrounding whitespace trimmed.
    ///
    /// This is not guaranteed to be free of internal whitespace or otherwise
    /// syntactically correct.
    pub name: &'a str,

    /// Raw header value.
    pub val: &'a [u8],
}

/// Iterator over all header fields in a request.
pub struct Headers<'a>(&'a [u8]);

impl<'a> Headers<'a> {
    /// Create a new `Headers` iterator over the given bytes, which must begin directly
    /// after the Request-Line CRLF.
    pub fn new(s: &'a [u8]) -> Self {
        Headers(s)
    }

    /// Retrieve the remaining bytes that haven't been processed.
    ///
    /// If called after the last yielded header, this slice will contain the beginning of
    /// the request body.
    pub fn into_inner(self) -> &'a [u8] { self.0 }
}

impl<'a> Iterator for Headers<'a> {
    type Item = Result<Header<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        let (line, rest) = match next_line(self.0) {
            Ok(x) => x,
            Err(e) => return Some(Err(e)),
        };

        self.0 = rest;

        // Headers are terminated by an empty line [RFC7230§3].
        if line.is_empty() {
            return None;
        }

        let (name, val) = match memchr(b':', line) {
            Some(idx) => line.split_at(idx),
            None => return Some(Err(Error::Syntax)),
        };

        let name = match std::str::from_utf8(name) {
            Ok(s) => s.trim(),
            Err(_) => return Some(Err(Error::Syntax)),
        };

        // Name must be nonempty [RFC7230§3.2].
        if name.is_empty() {
            return Some(Err(Error::Syntax));
        }

        // Skip past ':'.
        let val = &val[1..];

        Some(Ok(Header { name, val }))
    }
}

/// Consume CRLFs until the first non-CRLF character, returning a slice beginning at that
/// character.
fn skip_empty_lines<'a>(mut bytes: &'a [u8]) -> Result<&'a [u8]> {
    loop {
        match check_crlf(bytes) {
            Ok(rest) => bytes = rest,
            Err(Error::Partial) => return Err(Error::Partial),
            Err(Error::Syntax) => return Ok(bytes),
        }
    }
}

/// Retrieve the next chunk in the request, up to and not including the nearest CRLF.
fn next_line<'a>(bytes: &'a [u8]) -> Result<(&'a [u8], &'a [u8])> {
    let (line, rest) = match memchr(b'\r', bytes) {
        Some(idx) => bytes.split_at(idx),
        None => return Err(Error::Partial),
    };

    let rest = check_crlf(rest)?;

    Ok((line, rest))
}

/// Check if the given slice begins with CRLF and, if it does, return the slice
/// immediately after.
fn check_crlf<'a>(bytes: &'a [u8]) -> Result<&'a [u8]> {
    if bytes.len() < 2 {
        Err(Error::Partial)
    } else if bytes.starts_with(&b"\r\n"[..]) {
        // Skip over CRLF.
        Ok(&bytes[2..])
    } else {
        Err(Error::Syntax)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_request_line() {
        // Some testcases from httparse, MIT Copyright (c) 2015 Sean McArthur.

        let (req, rest) = RequestLine::new(b"GET / HTTP/1.1\r\n\r\n").unwrap();

        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "HTTP/1.1");

        assert_eq!(rest, &b"\r\n"[..]);

        let (req, rest) = RequestLine::new(
            b"GET / HTTP/1.1\r\nHost: foo.com\r\nCookie: \r\n\r\n"
        ).unwrap();

        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(rest, &b"Host: foo.com\r\nCookie: \r\n\r\n"[..]);

        let (req, rest) = RequestLine::new(
            b"GET / HTTP/1.1\r\nA: A\r\nB: B\r\nC: C\r\nD: D\r\n\r\n"
        ).unwrap();

        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(rest, &b"A: A\r\nB: B\r\nC: C\r\nD: D\r\n\r\n"[..]);

        let (req, rest) = RequestLine::new(
            b"GET / HTTP/1.1\r\nHost: foo.com\r\nUser-Agent: \xe3\x81\xb2\xe3/1.0\r\n\r\n",
        ).unwrap();

        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(rest, &b"Host: foo.com\r\nUser-Agent: \xe3\x81\xb2\xe3/1.0\r\n\r\n"[..]);

        let (req, rest) = RequestLine::new(b"GET / HTTP/1.1\r\n\r").unwrap();

        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(rest, &b"\r"[..]);

        assert_eq!(RequestLine::new(&b"GET / HTTP/1.1\nHost: foo.bar\n\n"[..]),
            Err(Error::Partial));

        let (req, rest) = RequestLine::new(&b"\r\n\r\nGET / HTTP/1.1\r\n\r\n"[..]).unwrap();

        assert_eq!(req.method, "GET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(rest, &b"\r\n"[..]);

        assert_eq!(RequestLine::new(&b"\n\nGET / HTTP/1.1\n\n"[..]), Err(Error::Partial));

        assert_eq!(RequestLine::new(&b"GET\n/ HTTP/1.1\r\nHost: foo.bar\r\n\r\n"[..]),
            Err(Error::Syntax));

        let (req, rest) = RequestLine::new(b"\n\n\nGET / HTTP/1.1\r\n\n").unwrap();

        assert_eq!(req.method, "\n\n\nGET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(rest, &b"\n"[..]);

        let (req, rest) = RequestLine::new(b"\r\n\nGET / H\tTTP/1.1\r\n\n").unwrap();

        assert_eq!(req.method, "\nGET");
        assert_eq!(req.target, "/");
        assert_eq!(req.version, "H\tTTP/1.1");
        assert_eq!(rest, &b"\n"[..]);

        assert_eq!(RequestLine::new(b"\r\n\rGET / HTTP/1.1\r\n\n"), Err(Error::Syntax));
        assert_eq!(RequestLine::new(b"GET /some path/ HTTP/1.1\r\n\r"), Err(Error::Syntax));
        assert_eq!(RequestLine::new(b"GET\r\n"), Err(Error::Syntax));
        assert_eq!(RequestLine::new(b"GET /\r\n"), Err(Error::Syntax));
        assert_eq!(RequestLine::new(b"GET / HTTP/1.1 \r\n"), Err(Error::Syntax));
        assert_eq!(RequestLine::new(b"GET /  \r\n"), Err(Error::Syntax));
        assert_eq!(RequestLine::new(b"GET / HTTP/1.1"), Err(Error::Partial));
        assert_eq!(RequestLine::new(b"GET / HTTP/1.1\r"), Err(Error::Partial));
        assert_eq!(RequestLine::new(b"GET / HTTP/1.1\n"), Err(Error::Partial));
    }

    #[test]
    fn test_headers() {
        let mut h = Headers::new(
            b"Content-Type: text/html\r\nContent-Length: 1337\r\n\r\nbody text"
        );
        let n = h.next().unwrap().unwrap();
        assert_eq!(n.name, "Content-Type");
        assert_eq!(n.val, b" text/html");
        let n = h.next().unwrap().unwrap();
        assert_eq!(n.name, "Content-Length");
        assert_eq!(n.val, b" 1337");
        assert!(h.next().is_none());
        assert_eq!(h.into_inner(), b"body text");

        let mut h = Headers::new(
            b"  Content-Type \t\t: text/html\r\n\r\nbody text"
        );
        let n = h.next().unwrap().unwrap();
        assert_eq!(n.name, "Content-Type");
        assert_eq!(n.val, b" text/html");
        assert!(h.next().is_none());
        assert_eq!(h.into_inner(), b"body text");

        let mut h = Headers::new(
            b"\tContent-Type \t\t: text/html\r\n\r\n"
        );
        let n = h.next().unwrap().unwrap();
        assert_eq!(n.name, "Content-Type");
        assert_eq!(n.val, b" text/html");
        assert!(h.next().is_none());
        assert_eq!(h.into_inner(), b"");

        let mut h = Headers::new(
            b"  Content-Type \t\t: text/html\n"
        );
        let n = h.next().unwrap();
        assert_eq!(n, Err(Error::Partial));
    }

    #[test]
    fn test_skip_empty_lines() {
        assert_eq!(skip_empty_lines(b"GET"), Ok(&b"GET"[..]));
        assert_eq!(skip_empty_lines(b"\r\n\r\nGET"), Ok(&b"GET"[..]));
        assert_eq!(skip_empty_lines(b"\r\n\rGET"), Ok(&b"\rGET"[..]));
        assert_eq!(skip_empty_lines(b"\nGET"), Ok(&b"\nGET"[..]));
    }

    #[test]
    fn test_next_line() {
        assert_eq!(next_line(b"abc\r\ndef"), Ok((&b"abc"[..], &b"def"[..])));
        assert_eq!(next_line(b"abc def\r\nghi"), Ok((&b"abc def"[..], &b"ghi"[..])));
        assert_eq!(next_line(b"abc\r\n"), Ok((&b"abc"[..], &b""[..])));
        assert_eq!(next_line(b"abc"), Err(Error::Partial));
        assert_eq!(next_line(b"abc\n"), Err(Error::Partial));
        assert_eq!(next_line(b"\r\ndef"), Ok((&b""[..], &b"def"[..])));
        assert_eq!(next_line(b""), Err(Error::Partial));
    }

    #[test]
    fn test_check_crlf() {
        assert_eq!(check_crlf(b"\r\nabc"), Ok(&b"abc"[..]));
        assert_eq!(check_crlf(b"\r"), Err(Error::Partial));
        assert_eq!(check_crlf(b""), Err(Error::Partial));
        assert_eq!(check_crlf(b"\n"), Err(Error::Partial));
        assert_eq!(check_crlf(b"\nabc"), Err(Error::Syntax));
        assert_eq!(check_crlf(b"abc\r\n"), Err(Error::Syntax));
    }
}
