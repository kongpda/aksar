"""Type stubs for the `aksar` Khmer text engine (Rust, via PyO3)."""

from typing import List, Literal, Optional, Sequence, Tuple

__version__: str

def normalize(text: str) -> str:
    """Canonical Khmer storage order (safe for typed text and OCR output)."""

def repair(text: str) -> str:
    """`normalize` plus OCR cleanup (invisible vowels, stutters)."""

def is_valid(text: str) -> bool:
    """True when the text has no broken clusters (renders without ◌)."""

def clusters(text: str) -> List[str]:
    """Khmer clusters (base + subscripts + vowel + signs) and other characters."""

def words(text: str) -> List[str]:
    """Splits Khmer text into dictionary words."""

def spell(token: str) -> Tuple[Literal["known", "corrected", "suspect"], str]:
    """Spell-checks one token against the dictionary (as `aksar spell` does):
    ('known', normalized), ('corrected', fix) or ('suspect', normalized),
    where normalized is the token in canonical order (aksar.normalize)."""

def check(token: str) -> Tuple[Literal["known", "corrected", "suspect"], str]:
    """Deprecated alias of `spell`; emits DeprecationWarning."""

IssueKind = Literal[
    "broken-cluster",
    "deprecated",
    "split-vowel",
    "dangling-coeng",
    "deep-stack",
    "subscript-after-vowel",
]

def issues(text: str) -> List[Tuple[IssueKind, int, int, str]]:
    """Validation issues, as `aksar check` reports them: (kind, start, end, message).

    `start`/`end` are character offsets: `text[start:end]` is the offending text.
    `broken-cluster` is the only error (renders with a dotted circle); the rest are
    warnings. Kind names match `aksar check --json`."""

def read(text: str, autocorrect: bool = True, user_words: Sequence[str] = ()) -> str:
    """Reads Khmer text and returns the document as a JSON string
    (pages → blocks → lines → words with parts, suggestions and flags).

    `user_words` (a list or tuple, not one string) are extra known words, e.g. names."""

def search_tokens(text: str) -> str:
    """Space-separated full-text index tokens (any Khmer substring is findable)."""

def fts_query(query: str) -> Optional[str]:
    """FTS5 MATCH expression for a user query, or None for an empty query.

    Keeps at most 64 terms, so a very long query matches more rows, never fewer."""
