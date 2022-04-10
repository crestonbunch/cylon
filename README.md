# Cylon

Cylon is a library for reading robots.txt files.

## Features

There is no universal standard for what rules a web crawler
is required to support in a robots.txt file. Cylon supports
the following directives (notably `Site-map` is missing):

- `User-agent`
- `Allow`
- `Disallow`
- `Crawl-Delay`

In addition, Cylon supports `*` as a wildcard character to
match any length substring of 0 or more characters, as well
as the `$` character to match the end of a path.

## Usage

Using Cylon is very simple. Simply create a new compiler
for your user agent, then compile the robots.txt file.

```rust
// You can use something like hyper or reqwest to download
// the robots.txt file instead.
let example_robots = r#"
User-agent: googlebot
Allow: /

User-agent: *
Disallow: /
"#
.as_bytes();

// Create a new compiler that compiles a robots.txt file looking for
// rules that apply to the "googlebot" user agent.
let compiler = Compiler::new("googlebot");
let cylon = compiler.compile(example_robots).await.unwrap();
assert_eq!(true, cylon.allow("/index.html"));
assert_eq!(true, cylon.allow("/directory"));

// Create a new compiler that compiles a robots.txt file looking for
// rules that apply to the "bing" user agent.
let complier = Compiler::new("bing");
let cylon = compiler.compile(example_robots).await.unwrap();
assert_eq!(false, cylon.allow("/index.html"));
assert_eq!(false, cylon.allow("/directory"));
```

## Contributing

Contributions are welcome! Please make a pull request. Issues may not
be addressed in a timely manner unless they expose fundamental issues
or security concerns.

## Implementation

### Async

This library uses an async API by default. This library does not assume
any async runtime so you can use it with any (tokio, async-std, etc.)

A synchronous API may be an optional feature in the future, but there
are no current plans to add one. If you need a synchronous API consider
adding one yourself (contributions are welcome).

### Performance

Cylon compiles robots.txt files into a NFA. This means it is well-suited
for web crawlers that need to use the same robots.txt file for multiple URLs.
Re-using the same compiled Cylon NFA will avoid repeated work. The NFA is
generally more efficient than naive matching. There are some degenerate
cases where it may perform worse.

Cylon minimizes random memory access when compiling and running the
NFA to maximize cache-locality.

Runtime performance for matching URLs is O(n^k). Suppose the length of the
input is n, the number of NFA states is m, and the average number of states
that match the input is k. In the worst case m == k. In practice m << k.
To trigger a degenerate case would require many wildcard matches. A typical
value of k may be between 2 and 5.

The number of NFA states m is proportional to the number of unique prefixes
in the robots.txt file. The number of states per input token k is proportional
to the number of times each unique prefix appears in the robots.txt file.
Wildcard matches also increase k. Special care is taken to avoid exponential
runtime when encounting repeated wildcard matches e.g. 'Allow: /\*\*\*\*\*'.
Multiple repeated wildcards are treated as a single wildcard.

It is important to understand the runtime tradeoffs with a naive approach.
The naive matching approach would require O(n \* p \* q) comparisons where
you match the n-length path against q p-length rules in the robots.txt file.
For situations where you need to match many inputs against a single robot.txt
file, this performance may be worse. In general k << q.

### (De-)serialization

This library uses serde to allow serializing/deserializing the compiled Cylon
NFA structs. This is useful e.g. if you need to cache the NFA in something like
Memcached or Redis. (Use a format like bincode or msgpack to convert it to
bytes first.)

### Error handling

Robots.txt files are more like guidelines than actual rules.

In general, Cylon tries not to cause errors for things that might be considered
an invalid robots.txt file, which means there are very few failure cases.

## License

MIT
