import json
import warnings

import aksar


def test_text_functions():
    assert aksar.normalize("កា្រ") == "ក្រា"
    assert aksar.is_valid("ក្ក") and not aksar.is_valid("ាក")
    assert aksar.clusters("កម្ពុជា") == ["ក", "ម្ពុ", "ជា"]
    assert aksar.words("ក្រសួងអប់រំ") == ["ក្រសួង", "អប់រំ"]
    assert aksar.spell("កម្ពជា") == ("corrected", "កម្ពុជា")
    assert aksar.spell("ឆ្ឃឞឞ")[0] == "suspect"
    assert "នៅ" in aksar.search_tokens("ខ្ញុំនៅផ្ទះ").split()
    assert aksar.fts_query("   ") is None


def test_check_is_a_deprecated_alias_of_spell():
    # `check` is a dictionary lookup (the CLI calls it `spell`); old callers keep
    # working but are told to move.
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        assert aksar.check("កម្ពជា") == aksar.spell("កម្ពជា")
    assert [w.category for w in caught] == [DeprecationWarning]
    assert "spell" in str(caught[0].message)
    assert caught[0].filename == __file__, "the warning points at the caller"


def test_issues_match_the_cli_check_kinds():
    assert aksar.issues("កម្ពុជា") == []
    # Offsets are characters, so slicing the str gives the offending text.
    text = "ក x\u17b6"
    [(kind, start, end, message)] = aksar.issues(text)
    assert (kind, text[start:end]) == ("broken-cluster", "\u17b6")
    assert message.startswith("broken cluster")
    # A split vowel (េ + ា for ោ) is a warning with its own kind.
    assert [i[0] for i in aksar.issues("កេា")] == ["split-vowel"]


def test_issue_offsets_on_long_text_with_many_issues():
    # Offsets stay character offsets after emoji and across many issues,
    # and a large text full of issues is fast (it used to be quadratic).
    text = "🙂" + " \u17b6" * 50000
    found = aksar.issues(text)
    assert len(found) == 50000
    for kind, start, end, _ in (found[0], found[-1]):
        assert kind == "broken-cluster" and text[start:end] == "\u17b6"


def test_read_returns_the_document():
    doc = json.loads(aksar.read("ក្រសួងអប់រំ កម្ពជា"))
    line = doc["pages"][0]["blocks"][0]["lines"][0]
    assert line["text"] == "ក្រសួងអប់រំ កម្ពុជា"
    assert line["words"][1]["corrected"] is True
    doc = json.loads(aksar.read("កម្ពជា", autocorrect=False))
    assert doc["pages"][0]["blocks"][0]["lines"][0]["words"][0]["suggestions"] == ["កម្ពុជា"]


def test_user_words_are_known():
    # A name not in the dictionary is flagged unless the caller lists it.
    word = lambda doc: json.loads(doc)["pages"][0]["blocks"][0]["lines"][0]["words"][0]
    assert word(aksar.read("ឆ្ឃឞឞ"))["unknown_word"] is True
    assert word(aksar.read("ឆ្ឃឞឞ", user_words=("ឆ្ឃឞឞ",)))["unknown_word"] is False


def test_user_words_must_be_a_sequence_not_one_string():
    # A bare str would otherwise be read as one word per character.
    try:
        aksar.read("ក", user_words="ក")
    except TypeError:
        return
    raise AssertionError("a str for user_words was accepted")


if __name__ == "__main__":
    test_text_functions()
    test_check_is_a_deprecated_alias_of_spell()
    test_issues_match_the_cli_check_kinds()
    test_issue_offsets_on_long_text_with_many_issues()
    test_read_returns_the_document()
    test_user_words_are_known()
    test_user_words_must_be_a_sequence_not_one_string()
    print("ok")
