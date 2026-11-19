"""Ticket spellings map to the keys the efficiency store keeps."""

from beryl_efficiency import ticket_ids


def test_capital_ids_map_to_the_earlier_keys() -> None:
    assert ticket_ids.prefixed("TQ44S.7") == "by-q44s.7"
    assert ticket_ids.prefixed("Tq44s.7") == "by-q44s.7"
    assert ticket_ids.prefixed("TCI-00031CED38") == "by-ci-00031ced38"
    assert ticket_ids.IDENTIFIER.fullmatch("TBEJE.61")
    assert ticket_ids.WORKTREE_TICKET.fullmatch("TBEJE.61")
    assert ticket_ids.canonical_spelling("adhoc2026") == "adhoc2026"


def test_epic_ids_map_to_keys_without_their_letter() -> None:
    assert ticket_ids.canonical_spelling("EQ44S.7") == "Eq44s.7"
    assert ticket_ids.prefixed("EQ44S.7") == "by-q44s.7"
    assert ticket_ids.IDENTIFIER.fullmatch("EQ44S.7")
    assert ticket_ids.WORKTREE_TICKET.fullmatch("EQ44S")
