"""Ticket spellings map to the keys the efficiency store keeps."""

from beryl_efficiency import ticket_ids


def test_capital_ids_map_to_the_earlier_keys() -> None:
    assert ticket_ids.prefixed("TQ44S.7") == "by-q44s.7"
    assert ticket_ids.prefixed("Tq44s.7") == "by-q44s.7"
    assert ticket_ids.prefixed("TCI-00031CED38") == "by-ci-00031ced38"
    assert ticket_ids.IDENTIFIER.fullmatch("TBEJE.61")
    assert ticket_ids.WORKTREE_TICKET.fullmatch("TBEJE.61")
    assert ticket_ids.canonical_spelling("adhoc2026") == "adhoc2026"
