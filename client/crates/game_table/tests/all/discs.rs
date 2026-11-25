//! Where a permanent's counter discs stand. The discs are drawn onto the card's face rather
//! than as elements of the tree, so the generic overlap check cannot see them. These tests hold
//! what that check would: no disc overlaps another, the plaque or the pile's count.

use game_table::table::badges::discs::Column;
use glam::Vec2;
use kit_base::data::BadgeTuning;

fn sizes() -> (BadgeTuning, f32, Vec2) {
    let data = crate::test_data::data();
    (
        data.badges,
        data.badges.em * data.card.width,
        data.card.half(),
    )
}

/// The bottom and top of a box with middle `at` and `size`.
fn span((at, size): (Vec2, Vec2)) -> (f32, f32) {
    (at.y - size.y / 2.0, at.y + size.y / 2.0)
}

#[test]
fn discs_stand_down_the_right_edge_apart_and_above_the_plaque() {
    let (tuning, em, corner) = sizes();
    let mut column = Column::new(&tuning, em, corner, 1);
    // A one-digit count, a two-digit count's pill, then two more.
    let discs: Vec<_> = [0.5, 3.0, 0.5, 0.5]
        .into_iter()
        .map_while(|width| column.next(width * em))
        .collect();
    assert_eq!(discs.len(), 4, "four kinds of counter fit on a card");
    let (_, first_top) = span(discs[0]);
    assert!(
        (corner.y - first_top - tuning.disc.top * em).abs() < 1e-4,
        "the first disc stands just below the name bar"
    );
    let plaque_top = -corner.y + (tuning.plaque.bottom + tuning.plaque.height) * em;
    for pair in discs.windows(2) {
        let (above_bottom, _) = span(pair[0]);
        let (_, below_top) = span(pair[1]);
        assert!(below_top < above_bottom, "{pair:?} overlap");
    }
    for &(at, size) in &discs {
        let right = at.x + size.x / 2.0;
        assert!((right - corner.x - tuning.disc.overhang * em).abs() < 1e-4);
        assert!(size.x >= size.y, "a disc is at least round");
        assert!(span((at, size)).0 > plaque_top, "{at} reaches the plaque");
    }
    assert!(
        discs[1].1.x > discs[1].1.y,
        "a wide count stretches its disc"
    );
}

#[test]
fn a_disc_that_would_reach_the_plaque_is_left_out() {
    let (tuning, em, corner) = sizes();
    let mut column = Column::new(&tuning, em, corner, 1);
    let placed = (0..20).map_while(|_| column.next(0.5 * em)).count();
    assert!(0 < placed && placed < 20);
    assert_eq!(column.next(0.5 * em), None);
}

#[test]
fn on_a_pile_the_discs_start_below_its_count() {
    let (tuning, em, corner) = sizes();
    let count_bottom = corner.y - (tuning.count.top + tuning.count.height) * em;
    let (_, top) = span(Column::new(&tuning, em, corner, 3).next(0.5 * em).unwrap());
    assert!(
        top < count_bottom,
        "the first disc overlaps the pile's count"
    );
}
