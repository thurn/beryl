//! Integration tests of game fixtures, playback, placement and frame checks.

#[path = "../../../../test_data.rs"]
mod test_data;
#[path = "../../../../test_draw_snapshot.rs"]
mod test_draw_snapshot;

mod agent;
mod badges;
mod badges_manual;
#[path = "../common/mod.rs"]
mod common;
mod effects;
mod effects_card;
mod effects_light;
mod effects_records;
mod hand_order;
mod light;
mod motion;
mod particles;
mod play;
mod relay;
mod signature;
mod sound;
mod stories;
mod table_compact;
mod table_compact_picks;
mod verbs_fly_draw;
mod verbs_leaving;
mod verbs_lift;
mod verbs_mark;
mod verbs_point;
mod verbs_shown;
mod verbs_streak;
mod verbs_strike;
mod verbs_tap;
