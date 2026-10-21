# quota-core

Provider logic behind [Quota](https://github.com/pinkpixel-dev/quota): read AI usage from the credentials an agent CLI already stored locally.

This is the shared crate that the Quota desktop app and [`quota-cli`](https://crates.io/crates/quota-cli) both build on. If you want the tool rather than the library, install `quota-cli`.

Seven providers are supported: Claude, Codex, Cursor, Antigravity, Grok, Kiro, and Kimi Code (`kimi`). Each one is a module with the same two halves. `local.rs` finds and parses the credentials, and `local_usage.rs` fetches usage and normalizes it into a `ProviderUsage`.

```rust
let reports = quota_core::providers::fetch_all().await;

for (provider, result) in reports {
    match result {
        Ok(usage) => println!("{provider}: {:?}", usage.compact_token()),
        Err(message) => eprintln!("{provider}: {message}"),
    }
}
```

`fetch_provider(name)` does one provider and returns `None` for a name that has no reader, which lets a caller tell "unknown provider" apart from "provider failed".

The Kimi provider reads OAuth credentials from `~/.kimi-code/credentials/kimi-code.json`, honoring `KIMI_CODE_HOME`. It fetches the default Kimi Code `/usages` endpoint and normalizes `used_ratio` into remaining percentages for `5h`, `Wk`, `Month`, and `Code`, preserving available reset times. Missing or invalid windows are skipped; no valid windows produces a `No usage data` note. Expired or rejected tokens must be renewed by Kimi Code. Legacy `~/.kimi` credentials, API keys, and custom service addresses are outside this provider's scope.

## Things worth knowing before you depend on this

Every reader is read-only. None of them refresh, rewrite, or rotate the files they read, since those tokens back the user's own agent sessions. Antigravity is the only exception, because Google does not rotate its refresh token, and even there nothing is written to disk.

Most of these APIs report how much has been *used*, not how much is left. Claude sends `utilization`, Codex `used_percent`, Cursor `totalPercentUsed`. `UsageWindow::remaining_percent` is always remaining, so every normalizer inverts and every provider has a test locking the direction.

Tokens never reach a log or a `Debug` output. Each credential struct has a hand-written `Debug` that redacts, and parse failures carry only a line and column, because serde's own message can quote the input it failed on.

This crate exists to serve Quota, and its API moves when Quota needs it to. Pin a version.

## License

Apache-2.0. Made with 💖 by Pink Pixel.
