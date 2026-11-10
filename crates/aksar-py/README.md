# aksar (Python)

Python bindings for [aksar](https://github.com/kongpda/aksar), a Khmer text engine written in
Rust. Everything runs locally; there are no network calls.

```python
import aksar, json

aksar.normalize("កា្រ")                    # 'ក្រា' (canonical storage order)
aksar.is_valid("ាក")                       # False (would render with ◌)
aksar.words("ព្រះរាជាណាចក្រកម្ពុជា")         # ['ព្រះរាជាណាចក្រ', 'កម្ពុជា']
aksar.spell("កម្ពជា")                      # ('corrected', 'កម្ពុជា')
aksar.issues("ក x\u17b6")                  # [('broken-cluster', 3, 4, 'broken cluster: ...')]
doc = json.loads(aksar.read("ទីក្រុងភ្នំពេញ កម្ពជា", user_words=["ភ្នំពេញ"]))
aksar.search_tokens("ខ្ញុំនៅផ្ទះ")             # tokens for a full-text index
aksar.fts_query("នៅផ្ទះ")                   # SQLite FTS5 MATCH expression
```

`spell` looks at one token against the bundled dictionary (the `aksar spell` command).
`issues` validates the spelling order of a whole text like the `aksar check` command and
returns `(kind, start, end, message)` tuples; `start` and `end` are character offsets, so
`text[start:end]` is the offending text, and `kind` is one of the names `aksar check --json`
uses (`broken-cluster` is the only error; the others are warnings).

`check(token)` is a deprecated alias of `spell` and emits a `DeprecationWarning`; it was
renamed because `aksar check` validates text while the old Python `check` looked a token up.

License: `(MIT OR Apache-2.0) AND Unicode-3.0` (the bundled ICU word list is under the
Unicode License v3).
